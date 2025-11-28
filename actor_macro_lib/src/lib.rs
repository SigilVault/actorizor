// #![feature(trace_macros)]
// trace_macros!(true);

extern crate proc_macro;

mod actorizor;
// mod pretty;

#[proc_macro_attribute]
pub fn actorize(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    actorizor::actorize(attr, item)
}
