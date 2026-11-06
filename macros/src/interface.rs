use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Fields, Ident, Item, ItemMod, ItemStruct, LitInt, Meta, Path, Token, Type, UseTree,
};

struct Operation {
    item: ItemStruct,
    conditions: Vec<Attribute>,
    code: LitInt,
    request: Type,
    response: Type,
    handler: Option<Ident>,
    call: Option<Ident>,
}

const RESERVED: &[&str] = &[
    "Interface",
    "Client",
    "Server",
    "Operation",
    "Request",
    "DecodedRequest",
    "Response",
    "DecodedResponse",
    "Handler",
    "dispatch",
];

fn check_name(name: &Ident) -> syn::Result<()> {
    if RESERVED.contains(&name.to_string().trim_start_matches("r#")) {
        return Err(syn::Error::new_spanned(
            name,
            "name is reserved for interface generation",
        ));
    }
    Ok(())
}

fn check_import(tree: &UseTree) -> syn::Result<()> {
    match tree {
        UseTree::Name(name) => check_name(&name.ident),
        UseTree::Rename(rename) => check_name(&rename.rename),
        UseTree::Path(path) => check_import(&path.tree),
        UseTree::Group(group) => {
            for item in &group.items {
                check_import(item)?;
            }
            Ok(())
        }
        UseTree::Glob(_) => Ok(()),
    }
}

fn check_item(item: &Item) -> syn::Result<()> {
    match item {
        Item::Const(item) => check_name(&item.ident),
        Item::Enum(item) => check_name(&item.ident),
        Item::Fn(item) => check_name(&item.sig.ident),
        Item::Mod(item) => check_name(&item.ident),
        Item::Static(item) => check_name(&item.ident),
        Item::Struct(item) => check_name(&item.ident),
        Item::Trait(item) => check_name(&item.ident),
        Item::TraitAlias(item) => check_name(&item.ident),
        Item::Type(item) => check_name(&item.ident),
        Item::Union(item) => check_name(&item.ident),
        Item::Use(item) => check_import(&item.tree),
        _ => Ok(()),
    }
}

// Propagate conditional availability, including nested cfg_attr, without copying
// unrelated attributes (such as derives) onto match arms and array elements.
fn condition(meta: &Meta) -> syn::Result<Option<Meta>> {
    if meta.path().is_ident("cfg") {
        return Ok(Some(meta.clone()));
    }
    if !meta.path().is_ident("cfg_attr") {
        return Ok(None);
    }
    let Meta::List(list) = meta else {
        return Err(syn::Error::new_spanned(meta, "expected cfg_attr arguments"));
    };
    let arguments = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    let mut arguments = arguments.iter();
    let predicate = arguments
        .next()
        .ok_or_else(|| syn::Error::new_spanned(meta, "missing cfg_attr predicate"))?;
    let mut conditions = Vec::new();
    for argument in arguments {
        if let Some(condition) = condition(argument)? {
            conditions.push(condition);
        }
    }
    if conditions.is_empty() {
        Ok(None)
    } else {
        Ok(Some(
            syn::parse_quote!(cfg_attr(#predicate, #(#conditions),*)),
        ))
    }
}

fn operation(
    mut item: ItemStruct,
    dispatch: bool,
    client: bool,
    maximum: u128,
) -> syn::Result<Operation> {
    if !matches!(item.fields, Fields::Unit)
        || !item.generics.params.is_empty()
        || item.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            &item,
            "operation requires a nongeneric unit struct",
        ));
    }
    let attributes: Vec<_> = item
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("operation"))
        .collect();
    if attributes.len() != 1 {
        return Err(syn::Error::new_spanned(
            &item,
            "expected one operation declaration",
        ));
    }
    let mut code = None;
    let mut request = None;
    let mut response = None;
    let mut handler = None;
    let mut call = None;
    attributes[0].parse_nested_meta(|meta| {
        if meta.path.is_ident("code") && code.is_none() {
            code = Some(meta.value()?.parse::<LitInt>()?);
        } else if meta.path.is_ident("request") && request.is_none() {
            request = Some(meta.value()?.parse::<Type>()?);
        } else if meta.path.is_ident("response") && response.is_none() {
            response = Some(meta.value()?.parse::<Type>()?);
        } else if meta.path.is_ident("handler") && handler.is_none() {
            handler = Some(meta.value()?.parse::<Ident>()?);
        } else if meta.path.is_ident("call") && call.is_none() {
            call = Some(meta.value()?.parse::<Ident>()?);
        } else {
            return Err(meta.error("unknown or repeated operation option"));
        }
        Ok(())
    })?;
    let code =
        code.ok_or_else(|| syn::Error::new_spanned(&item, "missing explicit operation code"))?;
    let number = code.base10_parse::<u128>()?;
    if number > maximum {
        return Err(syn::Error::new_spanned(
            code,
            "operation code exceeds tag range",
        ));
    }
    let request =
        request.ok_or_else(|| syn::Error::new_spanned(&item, "missing operation request type"))?;
    let response = response
        .ok_or_else(|| syn::Error::new_spanned(&item, "missing operation response type"))?;
    if dispatch != handler.is_some() {
        return Err(syn::Error::new_spanned(
            &item,
            "handler is required exactly when interface dispatch is enabled",
        ));
    }
    if client != call.is_some() {
        return Err(syn::Error::new_spanned(
            &item,
            "call is required exactly when interface client is enabled",
        ));
    }
    if let Some(call) = &call
        && ["new", "from_inner", "into_inner", "inner_mut"]
            .contains(&call.to_string().trim_start_matches("r#"))
    {
        return Err(syn::Error::new_spanned(
            call,
            "call name is reserved for client generation",
        ));
    }
    let mut conditions = Vec::new();
    for attribute in &item.attrs {
        if let Some(meta) = condition(&attribute.meta)? {
            conditions.push(syn::parse_quote!(#[#meta]));
        }
    }
    item.attrs.retain(|attr| !attr.path().is_ident("operation"));
    Ok(Operation {
        item,
        conditions,
        code: LitInt::new(&number.to_string(), code.span()),
        request,
        response,
        handler,
        call,
    })
}

pub fn expand(attr: TokenStream, input: TokenStream) -> syn::Result<TokenStream> {
    let mut tag = None;
    let mut dispatch = false;
    let mut client = false;
    let mut server = false;
    syn::meta::parser(|meta| {
        if meta.path.is_ident("tag") && tag.is_none() {
            tag = Some(meta.value()?.parse::<Ident>()?);
        } else if meta.path.is_ident("dispatch") && !dispatch {
            dispatch = true;
        } else if meta.path.is_ident("client") && !client {
            client = true;
        } else if meta.path.is_ident("server") && !server {
            server = true;
        } else {
            return Err(
                meta.error("expected tag = unsigned_integer and optional client, dispatch, server")
            );
        }
        Ok(())
    })
    .parse2(attr)?;
    let dispatch = dispatch || server;
    let tag = tag.ok_or_else(|| {
        syn::Error::new(proc_macro2::Span::call_site(), "missing interface tag type")
    })?;
    let maximum = match tag.to_string().as_str() {
        "u8" => u8::MAX as u128,
        "u16" => u16::MAX as u128,
        "u32" => u32::MAX as u128,
        "u64" => u64::MAX as u128,
        "u128" => u128::MAX,
        _ => {
            return Err(syn::Error::new_spanned(
                tag,
                "interface tag requires a fixed-width unsigned integer",
            ));
        }
    };
    let mut module: ItemMod = syn::parse2(input)?;
    let library = crate::path::library(&module.attrs)?;
    module.attrs.retain(|attr| !attr.path().is_ident("weave"));
    let (_, items) = module
        .content
        .take()
        .ok_or_else(|| syn::Error::new_spanned(&module, "interface requires an inline module"))?;
    let mut operations = Vec::new();
    let mut others = Vec::new();
    let mut codes = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut handlers = BTreeSet::new();
    let mut calls = BTreeSet::new();
    for item in items {
        check_item(&item)?;
        if let Item::Struct(item) = &item
            && item
                .attrs
                .iter()
                .any(|attr| attr.path().is_ident("operation"))
        {
            let operation = operation(item.clone(), dispatch, client, maximum)?;
            if !codes.insert(operation.code.base10_parse::<u128>()?) {
                return Err(syn::Error::new_spanned(
                    &operation.code,
                    "duplicate operation code",
                ));
            }
            if !names.insert(operation.item.ident.to_string()) {
                return Err(syn::Error::new_spanned(
                    &operation.item.ident,
                    "duplicate operation name",
                ));
            }
            if let Some(handler) = &operation.handler
                && !handlers.insert(handler.to_string().trim_start_matches("r#").to_owned())
            {
                return Err(syn::Error::new_spanned(handler, "duplicate handler name"));
            }
            if let Some(call) = &operation.call
                && !calls.insert(call.to_string().trim_start_matches("r#").to_owned())
            {
                return Err(syn::Error::new_spanned(call, "duplicate call name"));
            }
            operations.push(operation);
        } else {
            others.push(item);
        }
    }
    if operations.is_empty() {
        return Err(syn::Error::new_spanned(
            &module,
            "interface requires at least one operation",
        ));
    }
    let declarations = declarations(&operations, &library)?;
    let variants = operations.iter().map(|op| {
        let Operation {
            conditions,
            code,
            item,
            ..
        } = op;
        let name = &item.ident;
        quote!(#(#conditions)* #name = #code)
    });
    let request = message(
        &operations,
        &library,
        syn::parse_quote!(Request),
        syn::parse_quote!(DecodedRequest),
    );
    let response = message(
        &operations,
        &library,
        syn::parse_quote!(Response),
        syn::parse_quote!(DecodedResponse),
    );
    let dispatch = dispatch.then(|| dispatcher(&operations, &library));
    let client = client.then(|| client_wrapper(&operations, &library));
    let server = server.then(|| server_wrapper(&library));
    let attrs = &module.attrs;
    let visibility = &module.vis;
    let name = &module.ident;
    Ok(quote! {
        #(#attrs)* #visibility mod #name {
            #(#others)*
            #declarations
            pub struct Interface;
            impl #library::Contract for Interface {
                type Request = Request;
                type Response = Response;
            }
            #[derive(Clone, Copy, Debug, PartialEq, Eq, #library::Code)]
            #[weave(crate = #library)]
            #[repr(#tag)]
            pub enum Operation { #(#variants,)* }
            #request
            #response
            #dispatch
            #client
            #server
        }
    })
}

fn server_wrapper(library: &Path) -> TokenStream {
    quote! {
        pub struct Server<'a, R> {
            inner: #library::Server<'a, R>,
        }
        impl<'a, R> Server<'a, R> {
            pub fn new(receiver: R, request_buffer: &'a mut [u8], response_buffer: &'a mut [u8]) -> Self {
                Self::from_inner(#library::Server::new(receiver, request_buffer, response_buffer))
            }
            pub fn from_inner(inner: #library::Server<'a, R>) -> Self {
                Self { inner }
            }
            pub fn into_inner(self) -> #library::Server<'a, R> {
                self.inner
            }
            pub fn inner_mut(&mut self) -> &mut #library::Server<'a, R> {
                &mut self.inner
            }
        }
        impl<R: #library::FrameReceiver> Server<'_, R> {
            pub fn serve_once<H, S, P>(
                &mut self,
                handler: &mut H,
                route: impl ::core::ops::FnOnce(&DecodedRequest, R::Metadata) -> ::core::result::Result<S, P>,
            ) -> #library::server::ServeResult<R, S, P, H::Error>
            where
                H: Handler + ?Sized,
                S: #library::FrameSender,
            {
                self.inner.serve_once::<Interface, S, P, H::Error>(
                    route, |request| dispatch(handler, request),
                )
            }
        }
    }
}

fn client_wrapper(operations: &[Operation], library: &Path) -> TokenStream {
    let methods = operations.iter().map(|op| {
        let conditions = &op.conditions;
        let name = &op.item.ident;
        let call = op.call.as_ref().expect("validated call");
        quote! {
            #(#conditions)*
            pub fn #call<R: #library::correlation::ReplyReceiver + ?Sized>(
                &mut self,
                receiver: &mut R,
                request: <self::#name as #library::Contract>::Request,
            ) -> #library::request_reply::OperationResult<self::#name, S, R> {
                self.inner.call_operation::<self::#name, R>(receiver, request)
            }
        }
    });
    quote! {
        pub struct Client<'a, S> {
            inner: #library::Client<'a, S>,
        }
        impl<'a, S> Client<'a, S> {
            pub fn new(sender: S, request_buffer: &'a mut [u8], response_buffer: &'a mut [u8]) -> Self {
                Self::from_inner(#library::Client::new(sender, request_buffer, response_buffer))
            }
            pub fn from_inner(inner: #library::Client<'a, S>) -> Self {
                Self { inner }
            }
            pub fn into_inner(self) -> #library::Client<'a, S> {
                self.inner
            }
            pub fn inner_mut(&mut self) -> &mut #library::Client<'a, S> {
                &mut self.inner
            }
        }
        impl<S: #library::FrameSender> Client<'_, S> {
            #(#methods)*
        }
    }
}

fn declarations(operations: &[Operation], library: &Path) -> syn::Result<TokenStream> {
    let mut declarations = Vec::new();
    for op in operations {
        let Operation {
            item,
            conditions,
            request,
            response,
            ..
        } = op;
        let name = &item.ident;
        let other_responses = operations
            .iter()
            .filter(|other| other.item.ident != *name)
            .map(|other| {
                let conditions = &other.conditions;
                let name = &other.item.ident;
                quote!(#(#conditions)* DecodedResponse::#name(_) => ::core::option::Option::None)
            });
        let contract = crate::contract::expand(syn::parse_quote! {
            #[weave(crate = #library)]
            #[contract(request = #request, response = #response)]
            struct #name;
        })?;
        declarations.push(quote! {
            #item
            #(#conditions)* #contract
            #(#conditions)* impl #name {
                pub const TAG: Operation = Operation::#name;
            }
            #(#conditions)* impl #library::Operation for #name {
                type Interface = Interface;

                fn wrap_request(request: <Self as #library::Contract>::Request) -> Request {
                    Request::#name(request)
                }

                fn unwrap_response(response: DecodedResponse)
                    -> ::core::option::Option<<<Self as #library::Contract>::Response as #library::Codec>::Decoded>
                {
                    match response {
                        DecodedResponse::#name(value) => ::core::option::Option::Some(value),
                        #(#other_responses,)*
                    }
                }
            }
        });
    }
    Ok(quote!(#(#declarations)*))
}

fn message(operations: &[Operation], library: &Path, name: Ident, decoded: Ident) -> TokenStream {
    let mut variants = Vec::new();
    let mut decoded_variants = Vec::new();
    let mut sizes = Vec::new();
    let mut encoders = Vec::new();
    let mut decoders = Vec::new();
    for op in operations {
        let conditions = &op.conditions;
        let variant = &op.item.ident;
        let payload = quote!(<#variant as #library::Contract>::#name);
        variants.push(quote!(#(#conditions)* #variant(#payload)));
        decoded_variants
            .push(quote!(#(#conditions)* #variant(<#payload as #library::Codec>::Decoded)));
        sizes.push(quote!(#(#conditions)* <#payload as #library::Codec>::SIZE));
        encoders.push(quote! {
            #(#conditions)* Self::#variant(value) => {
                let tag = #library::Coded(Operation::#variant);
                let offset = #library::codec::encode(&tag, out)?;
                let length = #library::codec::encode(value, out.get_mut(offset..)?)?;
                offset.checked_add(length)
            }
        });
        decoders.push(quote! {
            #(#conditions)* Operation::#variant => {
                let (value, length) = #library::codec::decode::<#payload>(bytes.get(offset..)?)?;
                ::core::option::Option::Some((#decoded::#variant(value), offset.checked_add(length)?))
            }
        });
    }
    quote! {
        pub enum #name { #(#variants,)* }
        pub enum #decoded { #(#decoded_variants,)* }
        impl #library::Codec for #name {
            type Decoded = #decoded;
            const SIZE: #library::EncodedSize = #library::EncodedSize::sum(&[
                <#library::Coded<Operation> as #library::Codec>::SIZE,
                #library::EncodedSize::choice(&[#(#sizes,)*]),
            ]);
            fn encode(&self, out: &mut [u8]) -> ::core::option::Option<usize> {
                let out = #library::codec::output(<Self as #library::Codec>::SIZE, out);
                match self { #(#encoders,)* }
            }
            fn decode(bytes: &[u8]) -> ::core::option::Option<(#decoded, usize)> {
                let bytes = #library::codec::input(<Self as #library::Codec>::SIZE, bytes);
                let (tag, offset) = #library::codec::decode::<#library::Coded<Operation>>(bytes)?;
                match tag.0 { #(#decoders,)* }
            }
        }
    }
}

fn dispatcher(operations: &[Operation], library: &Path) -> TokenStream {
    let methods = operations.iter().map(|op| {
        let conditions = &op.conditions;
        let name = &op.item.ident;
        let handler = op.handler.as_ref().expect("validated handler");
        quote! {
            #(#conditions)*
            fn #handler(&mut self, request: <<#name as #library::Contract>::Request as #library::Codec>::Decoded)
                -> ::core::result::Result<<#name as #library::Contract>::Response, Self::Error>;
        }
    });
    let arms = operations.iter().map(|op| {
        let conditions = &op.conditions;
        let name = &op.item.ident;
        let handler = op.handler.as_ref().expect("validated handler");
        quote!(#(#conditions)* DecodedRequest::#name(value) => handler.#handler(value).map(Response::#name))
    });
    quote! {
        pub trait Handler {
            type Error;
            #(#methods)*
        }
        pub fn dispatch<H: Handler + ?Sized>(handler: &mut H, request: DecodedRequest)
            -> ::core::result::Result<Response, H::Error>
        {
            match request { #(#arms,)* }
        }
    }
}
