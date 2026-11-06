//! Code generation for the public Weave mechanisms.

mod code;
mod contract;
mod frame;
mod interface;
mod path;

#[cfg(test)]
mod tests;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

#[proc_macro_derive(Frame, attributes(frame, weave))]
pub fn frame(input: TokenStream) -> TokenStream {
    frame::expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Code, attributes(weave))]
pub fn code(input: TokenStream) -> TokenStream {
    code::expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(Contract, attributes(contract, weave))]
pub fn contract(input: TokenStream) -> TokenStream {
    contract::expand(parse_macro_input!(input as DeriveInput))
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Compose tagged codecs, named client calls, handler dispatch, and server execution.
#[proc_macro_attribute]
pub fn interface(attr: TokenStream, item: TokenStream) -> TokenStream {
    interface::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
