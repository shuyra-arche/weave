use syn::{Attribute, Path};

pub fn library(attrs: &[Attribute]) -> syn::Result<Path> {
    let mut path = None;
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("weave")) {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident("crate") || path.is_some() {
                return Err(meta.error("expected one `crate = path` option"));
            }
            path = Some(meta.value()?.parse::<Path>()?);
            Ok(())
        })?;
    }
    Ok(path.unwrap_or_else(|| syn::parse_quote!(::weave)))
}
