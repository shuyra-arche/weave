use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Expr, Field, Fields, GenericArgument, Ident, PathArguments, Type,
    WherePredicate,
};

enum Shape {
    Plain,
    Code,
    Prefixed {
        kind: Kind,
        prefix: Box<Type>,
        max: Expr,
    },
    Counted {
        count: Ident,
        fill: Expr,
    },
}

#[derive(Clone, Copy)]
enum Kind {
    Bytes,
    Text,
    Sequence,
}

fn shape(field: &Field) -> syn::Result<Shape> {
    let mut result = None;
    for attr in field
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("frame"))
    {
        attr.parse_nested_meta(|meta| {
            if result.is_some() {
                return Err(meta.error("only one frame encoding rule is allowed per field"));
            }
            if meta.path.is_ident("code") {
                result = Some(Shape::Code);
                return Ok(());
            }
            let kind = if meta.path.is_ident("bytes") {
                Kind::Bytes
            } else if meta.path.is_ident("text") {
                Kind::Text
            } else if meta.path.is_ident("sequence") {
                Kind::Sequence
            } else {
                return Err(meta.error("expected code, bytes(...), text(...), or sequence(...)"));
            };
            let mut prefix = None;
            let mut max = None;
            let mut count = None;
            let mut fill = None;
            meta.parse_nested_meta(|nested| {
                if nested.path.is_ident("prefix") && prefix.is_none() {
                    prefix = Some(nested.value()?.parse::<Type>()?);
                } else if nested.path.is_ident("max") && max.is_none() {
                    max = Some(nested.value()?.parse::<Expr>()?);
                } else if nested.path.is_ident("count") && count.is_none() {
                    count = Some(nested.value()?.parse::<Ident>()?);
                } else if nested.path.is_ident("fill") && fill.is_none() {
                    fill = Some(nested.value()?.parse::<Expr>()?);
                } else {
                    return Err(nested.error("unknown or repeated variable-length option"));
                }
                Ok(())
            })?;
            result =
                Some(match (prefix, max, count, fill) {
                    (Some(prefix), Some(max), None, None) => {
                        require_unsigned(&prefix)?;
                        Shape::Prefixed {
                            kind,
                            prefix: Box::new(prefix),
                            max,
                        }
                    }
                    (None, None, Some(count), Some(fill)) if matches!(kind, Kind::Sequence) => {
                        Shape::Counted { count, fill }
                    }
                    _ => return Err(meta.error(
                        "use prefix + max, or sequence(count + fill); these forms cannot be mixed",
                    )),
                });
            Ok(())
        })?;
        if result.is_none() {
            return Err(syn::Error::new_spanned(attr, "empty frame field rule"));
        }
    }
    Ok(result.unwrap_or(Shape::Plain))
}

fn require_unsigned(ty: &Type) -> syn::Result<()> {
    if let Type::Path(path) = ty
        && let Some(ident) = path.path.get_ident()
        && matches!(
            ident.to_string().as_str(),
            "u8" | "u16" | "u32" | "u64" | "u128"
        )
    {
        return Ok(());
    }
    Err(syn::Error::new_spanned(
        ty,
        "lengths require u8, u16, u32, u64, or u128",
    ))
}

fn vector_element(ty: &Type) -> syn::Result<&Type> {
    if let Type::Path(path) = ty
        && let Some(segment) = path.path.segments.last()
        && segment.ident == "Vec"
        && let PathArguments::AngleBracketed(arguments) = &segment.arguments
        && arguments.args.len() == 1
        && let Some(GenericArgument::Type(element)) = arguments.args.first()
    {
        return Ok(element);
    }
    Err(syn::Error::new_spanned(
        ty,
        "prefixed bytes and sequences require Vec<T>",
    ))
}

pub fn expand(input: DeriveInput) -> syn::Result<TokenStream> {
    let library = crate::path::library(&input.attrs)?;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "Frame requires a named-field struct",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input,
            "Frame requires named fields",
        ));
    };
    let mut max = None;
    for attr in input
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("frame"))
    {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("max") || max.is_some() {
                return Err(meta.error("expected one frame(max = ...) option"));
            }
            max = Some(meta.value()?.parse::<Expr>()?);
            Ok(())
        })?;
    }

    let out = Ident::new("__weave_out", Span::mixed_site());
    let bytes = Ident::new("__weave_bytes", Span::mixed_site());
    let used = Ident::new("__weave_used", Span::mixed_site());
    let length = Ident::new("__weave_length", Span::mixed_site());
    let mut sizes = Vec::new();
    let mut encoders = Vec::new();
    let mut decoders = Vec::new();
    let mut initializers = Vec::new();
    let mut bounds: Vec<WherePredicate> = Vec::new();
    let mut previous: Vec<(&Ident, &Type, Ident)> = Vec::new();

    for (index, field) in fields.named.iter().enumerate() {
        let name = field.ident.as_ref().expect("named field");
        let ty = &field.ty;
        let local = format_ident!("__weave_field_{index}", span = Span::mixed_site());
        let target = quote!(#out.get_mut(#used..)?);
        let source = quote!(#bytes.get(#used..)?);

        let (size, encoder, decoder) = match shape(field)? {
            Shape::Plain => {
                bounds.push(syn::parse_quote!(#ty: #library::Codec<Decoded = #ty>));
                (
                    quote!(<#ty as #library::Codec>::SIZE),
                    quote!(#library::codec::encode(&self.#name, #target)),
                    quote!(#library::codec::decode::<#ty>(#source)),
                )
            }
            Shape::Code => {
                bounds.push(syn::parse_quote!(#ty: #library::Code));
                bounds.push(syn::parse_quote!(<#ty as #library::Code>::Repr: #library::Codec<Decoded = <#ty as #library::Code>::Repr>));
                (
                    quote!(<<#ty as #library::Code>::Repr as #library::Codec>::SIZE),
                    quote!(#library::codec::encode(&<#ty as #library::Code>::code(&self.#name), #target)),
                    quote!({
                        let (value, length) = #library::codec::decode::<#library::Coded<#ty>>(#source)?;
                        ::core::option::Option::Some((value.0, length))
                    }),
                )
            }
            Shape::Prefixed { kind, prefix, max } => {
                let (element_size, encoder, decoder) = match kind {
                    Kind::Bytes => {
                        let element = vector_element(ty)?;
                        if !matches!(element, Type::Path(path) if path.path.is_ident("u8")) {
                            return Err(syn::Error::new_spanned(ty, "bytes requires Vec<u8>"));
                        }
                        (
                            quote!(#library::EncodedSize::Fixed(1)),
                            quote!(#library::codec::bounded::encode_bytes::<#prefix>(&self.#name, #max, #target)),
                            quote!(#library::codec::bounded::decode_owned_bytes::<#prefix>(#source, #max)),
                        )
                    }
                    Kind::Text => {
                        if !matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|segment| segment.ident == "String"))
                        {
                            return Err(syn::Error::new_spanned(ty, "text requires String"));
                        }
                        (
                            quote!(#library::EncodedSize::Fixed(1)),
                            quote!(#library::codec::bounded::encode_text::<#prefix>(&self.#name, #max, #target)),
                            quote!(#library::codec::bounded::decode_owned_text::<#prefix>(#source, #max)),
                        )
                    }
                    Kind::Sequence => {
                        let element = vector_element(ty)?;
                        bounds
                            .push(syn::parse_quote!(#element: #library::Codec<Decoded = #element>));
                        (
                            quote!(<#element as #library::Codec>::SIZE),
                            quote!(#library::codec::bounded::encode_sequence::<#prefix, #element>(&self.#name, #max, #target)),
                            quote!(#library::codec::bounded::decode_sequence::<#prefix, #element>(#source, #max)),
                        )
                    }
                };
                (
                    quote!(#library::codec::bounded::prefixed_size::<#prefix>(#element_size, #max)),
                    encoder,
                    decoder,
                )
            }
            Shape::Counted { count, fill } => {
                let Type::Array(array) = ty else {
                    return Err(syn::Error::new_spanned(ty, "count + fill requires [T; N]"));
                };
                let Some((_, count_type, count_local)) =
                    previous.iter().find(|(name, _, _)| **name == count)
                else {
                    return Err(syn::Error::new_spanned(
                        count,
                        "count must name an earlier field",
                    ));
                };
                require_unsigned(count_type)?;
                let element = &array.elem;
                let capacity = &array.len;
                bounds.push(syn::parse_quote!(#element: #library::Codec<Decoded = #element>));
                (
                    quote!(<#element as #library::Codec>::SIZE.up_to(#capacity)),
                    quote!({
                        let count = <usize as ::core::convert::TryFrom<#count_type>>::try_from(self.#count).ok()?;
                        #library::codec::bounded::encode_counted(&self.#name, count, #target)
                    }),
                    quote!({
                        let count = <usize as ::core::convert::TryFrom<#count_type>>::try_from(#count_local).ok()?;
                        #library::codec::bounded::decode_counted::<#element, #capacity>(#source, count, || #fill)
                    }),
                )
            }
        };
        sizes.push(size);
        encoders.push(quote! {
            let #length = #encoder?;
            #used = #used.checked_add(#length)?;
        });
        decoders.push(quote! {
            let (#local, #length): (#ty, usize) = #decoder?;
            #used = #used.checked_add(#length)?;
        });
        initializers.push(quote!(#name: #local));
        previous.push((name, ty, local));
    }

    let size = quote!(#library::EncodedSize::sum(&[#(#sizes),*]));
    let size = match max {
        Some(max) => quote!((#size).limited(#max)),
        None => size,
    };
    let name = &input.ident;
    let mut generics = input.generics.clone();
    generics.make_where_clause().predicates.extend(bounds);
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics #library::Codec for #name #ty_generics #where_clause {
            type Decoded = Self;
            const SIZE: #library::EncodedSize = #size;

            fn encode(&self, #out: &mut [u8]) -> ::core::option::Option<usize> {
                let #out = #library::codec::output(<Self as #library::Codec>::SIZE, #out);
                let mut #used = 0usize;
                #(#encoders)*
                (<Self as #library::Codec>::SIZE.accepts(#used)).then_some(#used)
            }

            fn decode(#bytes: &[u8]) -> ::core::option::Option<(Self, usize)> {
                let #bytes = #library::codec::input(<Self as #library::Codec>::SIZE, #bytes);
                let mut #used = 0usize;
                #(#decoders)*
                (<Self as #library::Codec>::SIZE.accepts(#used))
                    .then_some((Self { #(#initializers),* }, #used))
            }
        }
    })
}
