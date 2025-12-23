use convert_case::{Case, Casing as _};
use proc_macro2::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{Arm, FnArg, Ident, ImplItemFn, ItemImpl, Pat, ReturnType, Type, Variant, parse_quote};

const STD_QUEUE_DEPTH: usize = 10;

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
        let is_async = match value.sig.asyncness {
            Some(_) => true,
            None => false,
        };

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
                respond_to: tokio::sync::oneshot::Sender<#output>
            }
        );

        variant.to_token_stream()
    }

    fn to_handle_func(&self) -> TokenStream {
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
        let inp = self.inputs.clone();
        let output = self.output.clone();
        let fn_params = inp.iter().map(|i| i.to_handle_fn_params());
        let msg_pasthru = inp.iter().map(|i| i.to_msg_passthru());

        let handle_fn: ImplItemFn = parse_quote!(
            pub async fn #fn_name(&self, #(#fn_params)*) -> Result<#output, #handle_error_ident> {
                let (respond_to, response) = tokio::sync::oneshot::channel();
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

    fn to_constructor_func(&self) -> TokenStream {
        let actor_name = &self.actor_name.clone().unwrap();
        let fn_name = &self.fn_name;
        let inp = self.inputs.clone();
        let fn_params = inp.iter().map(|i| i.to_handle_fn_params());
        let constr_args = inp.iter().map(|i| i.to_msg_passthru());

        let (init_call, sig) = match self.is_async {
            true => (
                quote!(let mut actor = #actor_name::#fn_name(#(#constr_args),*).await),
                quote!(pub async fn #fn_name(#(#fn_params)*)),
            ),
            false => (
                quote!(let mut actor = #actor_name::#fn_name(#(#constr_args),*)),
                quote!(pub fn #fn_name(#(#fn_params)*)),
            ),
        };

        quote! {
            #sig -> Self {
                #init_call;
                Self::launch_actor(actor)
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

        let msg_enum: syn::ItemEnum = parse_quote! {
            enum #enum_ident {
                #(#variants,)*
            }
        };

        msg_enum.to_token_stream()
    }

    fn handle_token_stream(&self) -> TokenStream {
        let actor_ident = &self.actor_ident;
        let handle_ident = &self.handle_ident;
        let handle_funcs = self.actor_funcs.iter().map(|f| f.to_handle_func());
        let constructor_funcs = self
            .actor_constructors
            .iter()
            .map(|f| f.to_constructor_func());
        let message_enum_ident = &self.message_enum_ident;
        let qdepth = &self.qdepth;

        quote! {
            #[derive(Clone)]
            pub struct #handle_ident {
                sender: tokio::sync::mpsc::Sender<#message_enum_ident>,
            }

            impl #handle_ident {
                fn launch_actor(mut actor: #actor_ident) -> Self {
                    let (sender, receiver) = tokio::sync::mpsc::channel(#qdepth);
                    tokio::task::spawn(run_actor(actor, receiver));

                    Self { sender }
                }

                #(#constructor_funcs)*
                #(#handle_funcs)*
            }
        }
    }

    fn handle_message_fn(&self) -> syn::ImplItem {
        let message_enum_ident = &self.message_enum_ident;
        let error_enum_ident = &self.handle_error_ident;
        let funcs = self.actor_funcs.clone();
        let arms = funcs.iter().map(|f| f.to_handle_match());

        parse_quote! {
            async fn handle_msg(&mut self, msg: #message_enum_ident) -> Result<(), #error_enum_ident> {
                match msg {
                    #(#arms),*
                };

                Ok(())
            }
        }
    }

    fn run_actor_fn_stream(&self) -> TokenStream {
        let message_enum_ident = &self.message_enum_ident;
        let actor_ident = &self.actor_ident;

        quote! {
            async fn run_actor(
                mut actor: #actor_ident,
                mut receiver: tokio::sync::mpsc::Receiver<#message_enum_ident>,
            ) {
                while let Some(msg) = receiver.recv().await {
                    match actor.handle_msg(msg).await {
                        Ok(_) => continue,
                        Err(e) => eprintln!("error during actor message handling: {e:?}"),
                    };
                }
            }
        }
    }

    fn error_enum_stream(&self) -> TokenStream {
        let msg_enum_ident = &self.message_enum_ident;
        let handle_error_ident = &self.handle_error_ident;

        quote! {
            #[derive(thiserror::Error, Debug)]
            pub enum #handle_error_ident {
                #[error("send to actor error")]
                SendToActorError (#[from] tokio::sync::mpsc::error::SendError<#msg_enum_ident>),

                #[error("receive from actor error")]
                RespondToHandleError,

                #[error("receive from actor error")]
                RecvFromActorError(#[from] tokio::sync::oneshot::error::RecvError),
            }
        }
    }
}

impl From<ItemImpl> for Root {
    fn from(ast: ItemImpl) -> Self {
        let actor_ident = extract_impl_ident(&ast);
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
                        let return_ident = format!("{}", return_ident);
                        let actor_ident_str = format!("{}", actor_ident);

                        return_ident == "Self".to_string() || return_ident == actor_ident_str
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

        Root {
            orig_ast: ast,
            actor_ident,
            message_enum_ident,
            handle_ident,
            actor_funcs,
            actor_constructors,
            handle_error_ident,
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
    let impl_name = extract_impl_ident(item).to_string().to_case(Case::Pascal);
    pascal_ident(&format_ident!("{impl_name}"), suffix)
}

fn extract_impl_ident(item: &ItemImpl) -> Ident {
    let impl_name = item.self_ty.to_token_stream();
    parse_quote!(#impl_name)
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
    let ast = syn::parse::<ItemImpl>(item).expect("unable to get ast");
    let mut root = Root::from(ast);

    // This is awful but I dont fancy writing custom parser just for a single argument right now.
    let mut qdepth = STD_QUEUE_DEPTH;
    if attr.clone().into_iter().count() == 1 {
        for tok in attr {
            match tok {
                proc_macro::TokenTree::Literal(literal) => {
                    qdepth = format!("{}", literal).parse().unwrap();
                }
                _ => continue,
            }
        }
    }
    root.qdepth = qdepth;

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

    implementation.into()
}
