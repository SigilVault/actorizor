use convert_case::{Case, Casing as _};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{
    Arm, FnArg, GenericParam, Generics, Ident, ImplItemFn, ItemImpl, LitInt, Pat, ReturnType,
    Token, Type, Variant, parse_quote,
};

const STD_QUEUE_DEPTH: usize = 10;

// ---------------------------------------------------------------------------
// Attribute parsing
// ---------------------------------------------------------------------------
//
// Forms accepted:
//
//   #[actorize]
//   #[actorize(32)]           // positional qdepth
//   #[actorize(qdepth = 32)]  // named qdepth
//
// Supervision is provided via the generated `Handle::launch_with(actor, &S)`
// method, not via the attribute. See the parent crate's `Supervisor` trait
// + `TokioSpawn` / `TrackingSupervisor` types.

struct AttrArgs {
    qdepth: Option<usize>,
}

/// Parse a `LitInt` as the mailbox depth and reject `0` at expansion time.
/// `tokio::sync::mpsc::channel(0)` panics at runtime (the minimum bounded
/// capacity is 1), so catching it here turns a launch-time panic into a
/// clear compile error pointing at the literal.
fn parse_qdepth(lit: &LitInt) -> syn::Result<usize> {
    let v: usize = lit.base10_parse()?;
    if v == 0 {
        return Err(syn::Error::new(
            lit.span(),
            "actorize queue depth must be at least 1 (tokio::sync::mpsc::channel(0) panics)",
        ));
    }
    Ok(v)
}

impl Parse for AttrArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut qdepth = None;

        while !input.is_empty() {
            if input.peek(LitInt) {
                let lit: LitInt = input.parse()?;
                qdepth = Some(parse_qdepth(&lit)?);
            } else if input.peek(Ident) {
                let ident: Ident = input.parse()?;
                let _: Token![=] = input.parse()?;
                match ident.to_string().as_str() {
                    "qdepth" => {
                        let lit: LitInt = input.parse()?;
                        qdepth = Some(parse_qdepth(&lit)?);
                    }
                    other => {
                        return Err(syn::Error::new(
                            ident.span(),
                            format!(
                                "unknown actorize argument `{other}` (expected `qdepth`)"
                            ),
                        ));
                    }
                }
            } else {
                return Err(input.error(
                    "expected an integer literal (qdepth) or `qdepth = N`",
                ));
            }

            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        Ok(Self { qdepth })
    }
}

#[derive(Clone)]
struct FuncInput {
    inp_name: Ident,
    inp_ty: Type,
}

impl From<&FnArg> for FuncInput {
    fn from(value: &FnArg) -> Self {
        let FnArg::Typed(arg) = value else {
            panic!("Doesnt work for Self")
        };
        let inp_ty = *arg.ty.clone();

        let Pat::Ident(inp_name) = *arg.pat.clone() else {
            panic!("Unsupported function input parameter form")
        };
        let inp_name = inp_name.ident;

        Self { inp_name, inp_ty }
    }
}

impl FuncInput {
    fn to_enum_params(&self) -> TokenStream {
        let name = &self.inp_name;
        let ty = &self.inp_ty;

        quote!(#name: #ty,)
    }

    fn to_handle_fn_params(&self) -> TokenStream {
        self.to_enum_params()
    }

    fn to_msg_passthru(&self) -> TokenStream {
        let name = &self.inp_name;

        quote!(#name,)
    }
}

// ----

#[derive(Clone)]
struct ActorFunc {
    fn_name: Ident,
    msg_name: Ident,
    enum_name: Option<Ident>,
    actor_name: Option<Ident>,
    error_name: Option<Ident>,

    is_async: bool,
    inputs: Vec<FuncInput>,
    output: Type,
}

impl From<&ImplItemFn> for ActorFunc {
    fn from(value: &ImplItemFn) -> Self {
        let fn_name = value.sig.ident.clone();
        let msg_name = pascal_ident(&fn_name, None);
        let output = match value.sig.output.clone() {
            ReturnType::Type(_, t) => *t,
            ReturnType::Default => parse_quote!(()),
        };
        let is_async = value.sig.asyncness.is_some();

        let inputs = value
            .sig
            .inputs
            .iter()
            .filter_map(|f| match f {
                FnArg::Receiver(_) => None,
                FnArg::Typed(_) => Some(f.into()),
            })
            .collect();

        ActorFunc {
            fn_name,
            msg_name,
            inputs,
            output,
            is_async,
            enum_name: None,
            actor_name: None,
            error_name: None,
        }
    }
}

impl ActorFunc {
    fn to_enum_variant(&self) -> TokenStream {
        let msg_name = &self.msg_name;
        let inp = self.inputs.clone();
        let output = self.output.clone();
        let data = inp.iter().map(|i| i.to_enum_params());

        let variant: Variant = parse_quote!(
            #msg_name {
                #(#data)*
                respond_to: ::actorizor::__private::tokio::sync::oneshot::Sender<#output>
            }
        );

        variant.to_token_stream()
    }

    fn to_handle_func(&self, g: &ActorGenerics) -> TokenStream {
        let fn_name = &self.fn_name;
        let msg_name = &self.msg_name;
        let msg_enum_name = &self
            .enum_name
            .clone()
            .expect("message enum name not set for handle");
        let handle_error_ident = &self
            .error_name
            .clone()
            .expect("error enum not set for handle");
        let err_ty = &g.ty;
        let inp = self.inputs.clone();
        let output = self.output.clone();
        let fn_params = inp.iter().map(|i| i.to_handle_fn_params());
        let msg_pasthru = inp.iter().map(|i| i.to_msg_passthru());

        let handle_fn: ImplItemFn = parse_quote!(
            pub async fn #fn_name(&self, #(#fn_params)*) -> Result<#output, #handle_error_ident #err_ty> {
                let (respond_to, response) = ::actorizor::__private::tokio::sync::oneshot::channel();
                let msg = #msg_enum_name::#msg_name {
                    #(#msg_pasthru)*
                    respond_to,
                };

                self.sender.send(msg).await.map_err(|e| #handle_error_ident::from(e))?;
                let response = response.await.map_err(|e| #handle_error_ident::from(e))?;

                Ok(response)
            }
        );

        handle_fn.to_token_stream()
    }

    fn to_constructor_func(&self, g: &ActorGenerics) -> TokenStream {
        let actor_name = &self.actor_name.clone().unwrap();
        let fn_name = &self.fn_name;
        let inp = self.inputs.clone();
        let fn_params = inp.iter().map(|i| i.to_handle_fn_params());
        let constr_args = inp.iter().map(|i| i.to_msg_passthru());
        // Turbofish the actor's own constructor so `T` is pinned at the
        // call site — `MyActor::<T>::new()` — rather than left to fail
        // inference when the ctor's args don't mention `T`.
        let tf = &g.turbofish;

        let (init_call, sig) = match self.is_async {
            true => (
                quote!(let mut actor = #actor_name #tf::#fn_name(#(#constr_args)*).await),
                quote!(pub async fn #fn_name(#(#fn_params)*)),
            ),
            false => (
                quote!(let mut actor = #actor_name #tf::#fn_name(#(#constr_args)*)),
                quote!(pub fn #fn_name(#(#fn_params)*)),
            ),
        };

        quote! {
            #sig -> Self {
                #init_call;
                Self::launch_unsupervised(actor)
            }
        }
        .to_token_stream()
    }

    fn to_handle_match(&self) -> Arm {
        let msg_name = &self.msg_name;
        let msg_enum_name = &self
            .enum_name
            .clone()
            .expect("message enum name not set for handle");
        let inp = self.inputs.clone();
        let msg_passthru_var = inp.iter().map(|i| i.to_msg_passthru());
        let msg_passthru_fnc = msg_passthru_var.clone();
        let fn_name = &self.fn_name;
        let error_enum_ident = &self
            .error_name
            .clone()
            .expect("error enum name not set for handle");

        let exec_expr = match &self.is_async {
            true => quote!(let res = self.#fn_name(#(#msg_passthru_fnc)*).await),
            false => quote!(let res = self.#fn_name(#(#msg_passthru_fnc)*)),
        };

        let arm: Arm = parse_quote!(
            #msg_enum_name::#msg_name {#(#msg_passthru_var)* respond_to} => {
                #exec_expr;
                respond_to.send(res).map_err(|e| #error_enum_ident::RespondToHandleError)?;
            }
        );

        arm
    }
}

// ----

struct Root {
    orig_ast: ItemImpl,

    actor_ident: Ident,
    message_enum_ident: Ident,
    handle_ident: Ident,
    handle_error_ident: Ident,

    actor_funcs: Vec<ActorFunc>,
    actor_constructors: Vec<ActorFunc>,

    actor_generics: ActorGenerics,

    qdepth: usize,
}

impl Root {
    fn impl_token_stream(&self) -> TokenStream {
        let mut ast = self.orig_ast.clone();
        ast.items.push(self.handle_message_fn());
        ast.to_token_stream()
    }

    fn actor_msg_enum_token_stream(&self) -> TokenStream {
        let enum_ident = &self.message_enum_ident;
        let variants = self.actor_funcs.iter().map(|f| f.to_enum_variant());
        let decl = &self.actor_generics.decl;
        let where_ = &self.actor_generics.where_;

        // A hidden variant carrying `PhantomData<fn() -> (T, …)>` so that an
        // impl-generic param unused by any method still counts as "used" on
        // the enum (and transitively the handle + error enum). Never
        // constructed; `handle_msg` gets a matching unreachable arm.
        let phantom_variant = match &self.actor_generics.phantom {
            Some(p) => quote!( #[doc(hidden)] __ActorizorPhantom(#p), ),
            None => quote!(),
        };

        let msg_enum: syn::ItemEnum = parse_quote! {
            enum #enum_ident #decl #where_ {
                #(#variants,)*
                #phantom_variant
            }
        };

        msg_enum.to_token_stream()
    }

    fn handle_token_stream(&self) -> TokenStream {
        let actor_ident = &self.actor_ident;
        let handle_ident = &self.handle_ident;
        let handle_funcs = self
            .actor_funcs
            .iter()
            .map(|f| f.to_handle_func(&self.actor_generics));
        let constructor_funcs = self
            .actor_constructors
            .iter()
            .map(|f| f.to_constructor_func(&self.actor_generics));
        let message_enum_ident = &self.message_enum_ident;
        let qdepth = &self.qdepth;

        let decl = &self.actor_generics.decl; // <T: B>
        let ty = &self.actor_generics.ty; //    <T>
        let where_ = &self.actor_generics.where_; // where T: …
        let spawn = &self.actor_generics.spawn; // <T: B + Send + 'static>

        quote! {
            pub struct #handle_ident #decl #where_ {
                sender: ::actorizor::__private::tokio::sync::mpsc::Sender<#message_enum_ident #ty>,
                /// AbortHandle for the actor task. Cloned across every
                /// Handle clone; `abort()` fires through this. The Handle
                /// uses it for `is_alive` / `is_finished` queries too.
                abort: ::actorizor::__private::tokio::task::AbortHandle,
                /// Shared cooperative-shutdown signal. `shutdown()` calls
                /// `notify_one()` (sticky permit — survives until consumed,
                /// so a shutdown raced while `run_actor` is inside
                /// `handle_msg` is not lost); `run_actor`'s biased
                /// `select!` exits on the next `notified()`.
                shutdown: ::std::sync::Arc<::actorizor::__private::tokio::sync::Notify>,
            }

            // Hand-written (not `#[derive(Clone)]`): a derived impl would
            // add a bogus `T: Clone` bound. The handle only clones a
            // Sender/AbortHandle/Arc — all unconditionally `Clone` — so the
            // impl must hold no `T: Clone` requirement.
            impl #decl ::core::clone::Clone for #handle_ident #ty #where_ {
                fn clone(&self) -> Self {
                    Self {
                        sender: ::core::clone::Clone::clone(&self.sender),
                        abort: ::core::clone::Clone::clone(&self.abort),
                        shutdown: ::core::clone::Clone::clone(&self.shutdown),
                    }
                }
            }

            // The inherent impl uses the spawn-augmented bounds
            // (`T: … + Send + 'static`). Every way to obtain a handle goes
            // through a constructor / launch_with, all of which spawn the
            // actor, so a handle can only exist when those bounds hold —
            // putting them on the whole block keeps the generated code
            // uniform without actually restricting anything reachable.
            impl #spawn #handle_ident #ty #where_ {
                /// Construct the channel + shutdown signal + actor task,
                /// using the supplied supervisor to schedule the task.
                /// Returns the Handle wrapping the sender/abort/shutdown.
                pub fn launch_with<__ActorizorSup>(
                    actor: #actor_ident #ty,
                    sup: &__ActorizorSup,
                ) -> Self
                where
                    __ActorizorSup: ::actorizor::Supervisor,
                {
                    let (sender, receiver) = ::actorizor::__private::tokio::sync::mpsc::channel(#qdepth);
                    let shutdown = ::std::sync::Arc::new(::actorizor::__private::tokio::sync::Notify::new());
                    let abort = sup.spawn(
                        stringify!(#actor_ident),
                        run_actor(actor, receiver, shutdown.clone()),
                    );
                    Self { sender, abort, shutdown }
                }

                /// Unsupervised launch: schedules the actor task via
                /// `tokio::task::spawn` and discards the JoinHandle. Used
                /// internally by the generated constructors below.
                fn launch_unsupervised(actor: #actor_ident #ty) -> Self {
                    Self::launch_with(actor, &::actorizor::TokioSpawn)
                }

                /// Forcefully abort the actor task. The current message
                /// (if any) is dropped mid-poll; subsequent handle method
                /// calls will fail with `RecvFromActorError` once the
                /// oneshot Sender held inside the killed Msg is dropped.
                pub fn abort(&self) {
                    self.abort.abort();
                }

                /// Cooperatively signal the actor to stop. Uses
                /// `notify_one()` so the signal is a sticky permit: if
                /// `run_actor` is mid-`handle_msg` (not currently awaiting
                /// `notified()`) when this is called, the permit persists
                /// and the next loop iteration's `notified()` consumes it
                /// immediately. The loop exits without dropping the Sender
                /// clones, so this handle's methods still send successfully
                /// but `recv()` fails once the actor has exited.
                pub fn shutdown(&self) {
                    self.shutdown.notify_one();
                }

                /// Returns `true` if the actor task is still running.
                pub fn is_alive(&self) -> bool {
                    !self.abort.is_finished()
                }

                /// Returns `true` if the actor task has exited (clean,
                /// panic, or abort).
                pub fn is_finished(&self) -> bool {
                    self.abort.is_finished()
                }

                #(#constructor_funcs)*
                #(#handle_funcs)*
            }
        }
    }

    fn handle_message_fn(&self) -> syn::ImplItem {
        let message_enum_ident = &self.message_enum_ident;
        let error_enum_ident = &self.handle_error_ident;
        let ty = &self.actor_generics.ty;
        let funcs = self.actor_funcs.clone();
        let arms = funcs.iter().map(|f| f.to_handle_match());

        // The hidden `__ActorizorPhantom` variant (emitted only when the
        // actor has generic params) is never constructed, but the match
        // must stay exhaustive.
        let phantom_arm = if self.actor_generics.phantom.is_some() {
            quote! {
                #message_enum_ident::__ActorizorPhantom(_) => {
                    ::core::unreachable!("__ActorizorPhantom is never constructed")
                }
            }
        } else {
            quote!()
        };

        parse_quote! {
            async fn handle_msg(
                &mut self,
                msg: #message_enum_ident #ty,
            ) -> Result<(), #error_enum_ident #ty> {
                match msg {
                    #(#arms),*
                    #phantom_arm
                };

                Ok(())
            }
        }
    }

    fn run_actor_fn_stream(&self) -> TokenStream {
        let message_enum_ident = &self.message_enum_ident;
        let actor_ident = &self.actor_ident;
        let spawn = &self.actor_generics.spawn; // <T: B + Send + 'static>
        let ty = &self.actor_generics.ty; //      <T>
        let where_ = &self.actor_generics.where_;

        quote! {
            async fn run_actor #spawn (
                mut actor: #actor_ident #ty,
                mut receiver: ::actorizor::__private::tokio::sync::mpsc::Receiver<#message_enum_ident #ty>,
                shutdown: ::std::sync::Arc<::actorizor::__private::tokio::sync::Notify>,
            ) #where_ {
                loop {
                    ::actorizor::__private::tokio::select! {
                        biased;
                        _ = shutdown.notified() => break,
                        maybe_msg = receiver.recv() => match maybe_msg {
                            None => break,
                            Some(msg) => match actor.handle_msg(msg).await {
                                Ok(_) => continue,
                                Err(e) => ::actorizor::__private::tracing::warn!(
                                    actor = stringify!(#actor_ident),
                                    error = ?e,
                                    "actor message handling failed",
                                ),
                            },
                        },
                    }
                }
            }
        }
    }

    fn error_enum_stream(&self) -> TokenStream {
        let msg_enum_ident = &self.message_enum_ident;
        let handle_error_ident = &self.handle_error_ident;
        let decl = &self.actor_generics.decl; // <T: B>
        let ty = &self.actor_generics.ty; //    <T>
        let where_ = &self.actor_generics.where_;

        // All error impls are hand-written so generated code pulls in no
        // extra crates (no `thiserror`) AND so the generic case is sound:
        // a `#[derive(Debug)]` on `#handle_error_ident<T>` would add a
        // bogus `T: Debug` bound (same perfect-derive trap as the handle's
        // `Clone`). The wrapped tokio errors
        // (`mpsc::error::SendError`, `oneshot::error::RecvError`) impl
        // `Debug`/`Display`/`Error` unconditionally, so a manual `Debug`
        // that defers to them is sound for any message type.
        quote! {
            pub enum #handle_error_ident #decl #where_ {
                SendToActorError(
                    ::actorizor::__private::tokio::sync::mpsc::error::SendError<#msg_enum_ident #ty>,
                ),
                RespondToHandleError,
                RecvFromActorError(
                    ::actorizor::__private::tokio::sync::oneshot::error::RecvError,
                ),
            }

            impl #decl ::core::fmt::Debug for #handle_error_ident #ty #where_ {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::SendToActorError(e) =>
                            f.debug_tuple("SendToActorError").field(e).finish(),
                        Self::RespondToHandleError =>
                            f.write_str("RespondToHandleError"),
                        Self::RecvFromActorError(e) =>
                            f.debug_tuple("RecvFromActorError").field(e).finish(),
                    }
                }
            }

            impl #decl ::core::fmt::Display for #handle_error_ident #ty #where_ {
                fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                    match self {
                        Self::SendToActorError(_) => f.write_str("send to actor error"),
                        Self::RespondToHandleError => f.write_str("receive from actor error"),
                        Self::RecvFromActorError(_) => f.write_str("receive from actor error"),
                    }
                }
            }

            impl #decl ::std::error::Error for #handle_error_ident #ty #where_ {
                fn source(
                    &self,
                ) -> ::core::option::Option<&(dyn ::std::error::Error + 'static)> {
                    match self {
                        Self::SendToActorError(e) => ::core::option::Option::Some(e),
                        Self::RespondToHandleError => ::core::option::Option::None,
                        Self::RecvFromActorError(e) => ::core::option::Option::Some(e),
                    }
                }
            }

            impl #decl ::core::convert::From<
                ::actorizor::__private::tokio::sync::mpsc::error::SendError<#msg_enum_ident #ty>,
            > for #handle_error_ident #ty #where_ {
                fn from(
                    e: ::actorizor::__private::tokio::sync::mpsc::error::SendError<#msg_enum_ident #ty>,
                ) -> Self {
                    Self::SendToActorError(e)
                }
            }

            impl #decl ::core::convert::From<
                ::actorizor::__private::tokio::sync::oneshot::error::RecvError,
            > for #handle_error_ident #ty #where_ {
                fn from(
                    e: ::actorizor::__private::tokio::sync::oneshot::error::RecvError,
                ) -> Self {
                    Self::RecvFromActorError(e)
                }
            }
        }
    }
}

impl From<ItemImpl> for Root {
    fn from(ast: ItemImpl) -> Self {
        let actor_ident = extract_base_ident(&ast);
        let message_enum_ident = impl_to_ident(&ast, Some("ActorMsg"));
        let handle_ident = impl_to_ident(&ast, Some("Handle"));
        let handle_error_ident = impl_to_ident(&ast, Some("HandleError"));

        let actor_funcs = extract_functions_raw(&ast, true)
            .map(|f| {
                let mut f: ActorFunc = f.into();
                f.enum_name = Some(message_enum_ident.clone());
                f.actor_name = Some(actor_ident.clone());
                f.error_name = Some(handle_error_ident.clone());
                f
            })
            .collect();

        // We assume a constructor is any member function which has a return token of either Self or matching the
        // name of the actor.
        let actor_constructors = extract_functions_raw(&ast, false)
            .filter(|f| match &f.sig.output {
                ReturnType::Default => false,
                ReturnType::Type(_, ty) => match *ty.clone() {
                    Type::Path(path) => {
                        let default_ident = format_ident!("NO_IDENT");
                        let return_ident = match path.path.get_ident() {
                            Some(ident) => ident,
                            None => &default_ident,
                        };
                        let return_ident = return_ident.to_string();
                        let actor_ident_str = actor_ident.to_string();

                        return_ident == "Self" || return_ident == actor_ident_str
                    }
                    _ => false,
                },
            })
            .map(|f| {
                let mut f: ActorFunc = f.into();
                f.enum_name = Some(message_enum_ident.clone());
                f.actor_name = Some(actor_ident.clone());
                f.error_name = Some(handle_error_ident.clone());
                f
            })
            .collect();

        let actor_generics = ActorGenerics::from_generics(&ast.generics);

        Root {
            orig_ast: ast,
            actor_ident,
            message_enum_ident,
            handle_ident,
            actor_funcs,
            actor_constructors,
            handle_error_ident,
            actor_generics,
            qdepth: STD_QUEUE_DEPTH,
        }
    }
}

// ----

fn pascal_ident(item: &Ident, suffix: Option<&str>) -> Ident {
    let impl_name = item.to_string().to_case(Case::Pascal);
    let suffix = suffix.unwrap_or_default();
    let impl_name = format!("{impl_name}{suffix}");
    format_ident!("{}", impl_name)
}

fn impl_to_ident(item: &ItemImpl, suffix: Option<&str>) -> Ident {
    let impl_name = extract_base_ident(item).to_string().to_case(Case::Pascal);
    pascal_ident(&format_ident!("{impl_name}"), suffix)
}

/// The bare type name of the impl's self type — `MyActor` for both
/// `impl MyActor` and `impl<T> MyActor<T>`. Used only for *naming* the
/// generated types (`MyActorHandle`, `MyActorActorMsg`, …). The generic
/// arguments are handled separately by [`ActorGenerics`]; conflating the
/// name with the full generic type is what made the original generator
/// fragile (case-converting `MyActor < T >` produced garbage idents).
fn extract_base_ident(item: &ItemImpl) -> Ident {
    match &*item.self_ty {
        Type::Path(tp) => tp
            .path
            .segments
            .last()
            .map(|seg| seg.ident.clone())
            .unwrap_or_else(|| format_ident!("NO_IDENT")),
        other => {
            // e.g. `impl (Foo, Bar)` — not a named type; fall back to the
            // token stream so the (already-bad) input still produces a
            // diagnostic rather than a panic.
            let ts = other.to_token_stream();
            parse_quote!(#ts)
        }
    }
}

/// Does this trait-bound path name `Send` (bare or `…::marker::Send`)?
fn path_is_send(path: &syn::Path) -> bool {
    path.segments
        .last()
        .map(|s| s.ident == "Send")
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Generics plumbing
// ---------------------------------------------------------------------------
//
// One source of truth that vends every generic form the codegen needs, so
// they never drift apart:
//
//   decl     — `<T: B>`        impl/enum/struct headers + error impls
//   ty       — `<T>`           applied after a generated type name
//   where_   — `where T: …`    carried onto every generated item
//   spawn    — `<T: B + Send + 'static>`  run_actor / launch_with actor arg
//   turbofish— `::<T>`         the constructor's call into the actor ctor
//   phantom  — `PhantomData<fn() -> (T, [(); N])>`  hidden msg-enum variant
//              so an impl-generic param unused by any method still counts
//              as "used" on the message enum (and transitively the handle
//              + error enum, which hold `…<MsgEnum<T>>`).
//
// Lifetimes parameters and method-level generics are rejected up front
// (see `validate_generics`), so only type + const params reach here.
struct ActorGenerics {
    decl: TokenStream,
    ty: TokenStream,
    where_: TokenStream,
    spawn: TokenStream,
    turbofish: TokenStream,
    /// `Some(PhantomData<…>)` when the impl has any type/const params.
    phantom: Option<TokenStream>,
}

impl ActorGenerics {
    fn from_generics(g: &Generics) -> Self {
        let (decl_g, ty_g, where_g) = g.split_for_impl();
        let decl = quote!(#decl_g);
        let ty = quote!(#ty_g);
        let where_ = quote!(#where_g);
        let turbofish = {
            let tf = ty_g.as_turbofish();
            quote!(#tf)
        };

        // Augmented generics for the spawn path: every type param also
        // needs `Send + 'static` because the actor future is handed to a
        // `Supervisor` that `tokio::spawn`s it. Add each bound ONLY if the
        // user hasn't already written it (on the param or in the
        // where-clause) — a blind push produces a "bound defined in more
        // than one place" warning that would surface on the user's
        // `#[actorize]`.
        let mut aug = g.clone();
        for p in &mut aug.params {
            if let GenericParam::Type(tp) = p {
                let (mut has_send, mut has_static) = (false, false);
                for b in &tp.bounds {
                    match b {
                        syn::TypeParamBound::Trait(tb) => {
                            if path_is_send(&tb.path) {
                                has_send = true;
                            }
                        }
                        syn::TypeParamBound::Lifetime(lt) => {
                            if lt.ident == "static" {
                                has_static = true;
                            }
                        }
                        _ => {}
                    }
                }
                // Also honour bounds written in the where-clause for this
                // exact param.
                if let Some(wc) = &g.where_clause {
                    for pred in &wc.predicates {
                        if let syn::WherePredicate::Type(pt) = pred
                            && matches!(
                                &pt.bounded_ty,
                                syn::Type::Path(p) if p.path.is_ident(&tp.ident)
                            )
                        {
                            for b in &pt.bounds {
                                match b {
                                    syn::TypeParamBound::Trait(tb)
                                        if path_is_send(&tb.path) =>
                                    {
                                        has_send = true
                                    }
                                    syn::TypeParamBound::Lifetime(lt)
                                        if lt.ident == "static" =>
                                    {
                                        has_static = true
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
                if !has_send {
                    tp.bounds.push(parse_quote!(::core::marker::Send));
                }
                if !has_static {
                    tp.bounds.push(parse_quote!('static));
                }
            }
        }
        let (spawn_g, _, _) = aug.split_for_impl();
        let spawn = quote!(#spawn_g);

        // Collect type + const param idents for the phantom. A type param
        // is bound via `T`; a const param via `[(); N]` (a type that uses
        // the const). Lifetimes are already rejected.
        let mut tys: Vec<TokenStream> = Vec::new();
        for p in &g.params {
            match p {
                GenericParam::Type(tp) => {
                    let id = &tp.ident;
                    tys.push(quote!(#id));
                }
                GenericParam::Const(cp) => {
                    let id = &cp.ident;
                    tys.push(quote!([(); #id]));
                }
                GenericParam::Lifetime(_) => {}
            }
        }
        let phantom = if tys.is_empty() {
            None
        } else {
            Some(quote!(::core::marker::PhantomData<fn() -> ( #(#tys ,)* )>))
        };

        Self {
            decl,
            ty,
            where_,
            spawn,
            turbofish,
            phantom,
        }
    }
}

/// Reject the two generic shapes the actor model cannot express:
///
/// - **Lifetime parameters** on the impl. A `MyActor<'a>` borrowing
///   something cannot be `tokio::spawn`ed (the task would outlive the
///   borrow); only `'static` is meaningful, and a lifetime *parameter* is
///   inherently not `'static`.
/// - **Method-level generics** (`pub fn foo<U>(…)`). An enum variant
///   cannot carry a generic that isn't a parameter of the enum; supporting
///   this would require per-message type erasure. Out of scope.
fn validate_generics(ast: &ItemImpl) -> syn::Result<()> {
    for p in &ast.generics.params {
        if let GenericParam::Lifetime(lp) = p {
            return Err(syn::Error::new_spanned(
                lp,
                "actorize: lifetime parameters are not supported — an \
                 actor task is spawned and must be 'static (the actor may \
                 still hold 'static references internally)",
            ));
        }
    }
    for item in &ast.items {
        if let syn::ImplItem::Fn(f) = item
            && let Some(p) = f.sig.generics.params.first()
        {
            return Err(syn::Error::new_spanned(
                p,
                "actorize: generic methods are not supported — only \
                 impl-level type/const generics. Move the parameter to \
                 the impl, or monomorphise at the call site.",
            ));
        }
    }
    Ok(())
}

fn extract_functions_raw(item: &ItemImpl, only_methods: bool) -> impl Iterator<Item = &ImplItemFn> {
    item.items
        .iter()
        .filter_map(|f| match f {
            syn::ImplItem::Fn(impl_item_fn) => Some(impl_item_fn),
            _ => None,
        })
        .filter(move |f| match &f.vis {
            syn::Visibility::Public(_) => true,
            // TODO: "Restricted" means pub(super) or similar.  We should really match those restrictions.
            syn::Visibility::Restricted(_vis_restricted) => true,
            syn::Visibility::Inherited => false,
        })
        .filter(move |f| match only_methods {
            true => f.sig.receiver().is_some(),
            false => true,
        })
}

// ----

pub fn actorize(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    // Surface parse failures as a `compile_error!` at the offending span
    // (a panic in a proc-macro produces a far worse "proc-macro panicked"
    // diagnostic with no source location).
    match actorize_inner(attr.into(), item.into()) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn actorize_inner(
    attr: proc_macro2::TokenStream,
    item: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let ast = syn::parse2::<ItemImpl>(item)?;
    validate_generics(&ast)?;
    let mut root = Root::from(ast);

    let args: AttrArgs = if attr.is_empty() {
        AttrArgs { qdepth: None }
    } else {
        syn::parse2(attr)?
    };

    if let Some(q) = args.qdepth {
        root.qdepth = q;
    }

    let error_enum_stream = root.error_enum_stream();
    let impl_token_stream = root.impl_token_stream();
    let actor_msg_enum_token_stream = root.actor_msg_enum_token_stream();
    let handle_token_stream = root.handle_token_stream();
    let run_actor_fn_stream = root.run_actor_fn_stream();

    let implementation = quote! {
        #error_enum_stream
        #impl_token_stream
        #actor_msg_enum_token_stream
        #handle_token_stream
        #run_actor_fn_stream
    };

    #[cfg(feature = "diagout")]
    eprintln!(
        "{}",
        crate::pretty::pretty_print(&implementation)
            .unwrap_or("Error during pretty print".to_owned())
    );

    Ok(implementation)
}
