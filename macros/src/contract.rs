use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Type};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let library = crate::path::library(&input.attrs)?;
    if !matches!(input.data, Data::Struct(_)) {
        return Err(syn::Error::new_spanned(
            &input,
            "Contract requires a struct",
        ));
    }
    let mut request = None;
    let mut response = None;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("contract"))
    {
        attr.parse_nested_meta(|meta| {
            let slot = if meta.path.is_ident("request") {
                &mut request
            } else if meta.path.is_ident("response") {
                &mut response
            } else {
                return Err(meta.error("expected request or response"));
            };
            if slot.is_some() {
                return Err(meta.error("duplicate contract option"));
            }
            *slot = Some(meta.value()?.parse::<Type>()?);
            Ok(())
        })?;
    }
    let request =
        request.ok_or_else(|| syn::Error::new_spanned(&input, "missing contract request"))?;
    let response =
        response.ok_or_else(|| syn::Error::new_spanned(&input, "missing contract response"))?;
    let name = &input.ident;
    let mut generics = input.generics.clone();
    let predicates = &mut generics.make_where_clause().predicates;
    predicates.push(syn::parse_quote!(#request: #library::Codec));
    predicates.push(syn::parse_quote!(#response: #library::Codec));
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics #library::Contract for #name #ty_generics #where_clause {
            type Request = #request;
            type Response = #response;
        }
    })
}
