use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Ident, Token, punctuated::Punctuated};

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let library = crate::path::library(&input.attrs)?;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input, "Code requires an enum"));
    };
    if data.variants.is_empty() || !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input,
            "Code requires a nonempty, nongeneric enum",
        ));
    }
    let mut repr = None;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("repr"))
    {
        for ident in attr.parse_args_with(Punctuated::<Ident, Token![,]>::parse_terminated)? {
            if ident == "C" {
                continue;
            }
            if !matches!(
                ident.to_string().as_str(),
                "u8" | "u16" | "u32" | "u64" | "u128" | "i8" | "i16" | "i32" | "i64" | "i128"
            ) || repr.is_some()
            {
                return Err(syn::Error::new_spanned(
                    ident,
                    "Code requires one fixed-width integer repr",
                ));
            }
            repr = Some(ident);
        }
    }
    let repr = repr.ok_or_else(|| {
        syn::Error::new_spanned(
            &input,
            "Code requires #[repr(u8)] or another fixed-width integer repr",
        )
    })?;
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) || variant.discriminant.is_none() {
            return Err(syn::Error::new_spanned(
                variant,
                "each Code variant must be fieldless with an explicit value",
            ));
        }
    }
    let name = &input.ident;
    let variants: Vec<_> = data.variants.iter().map(|variant| &variant.ident).collect();
    // Rust checks duplicate values and repr range in the original enum. Referring
    // to discriminants also supports constant expressions without re-evaluation.
    Ok(quote! {
        impl #library::Code for #name {
            type Repr = #repr;
            fn code(&self) -> #repr {
                match self { #(Self::#variants => Self::#variants as #repr,)* }
            }
            fn from_code(value: #repr) -> ::core::option::Option<Self> {
                #(if value == Self::#variants as #repr {
                    return ::core::option::Option::Some(Self::#variants);
                })*
                ::core::option::Option::None
            }
        }
    })
}
