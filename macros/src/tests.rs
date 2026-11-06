use syn::parse_quote;

#[test]
fn code_requires_explicit_fixed_width_representation() {
    let input = parse_quote!(
        enum Bad {
            A = 1,
        }
    );
    assert!(
        crate::code::expand(input)
            .unwrap_err()
            .to_string()
            .contains("repr")
    );
    let input = parse_quote!(
        #[repr(usize)]
        enum Bad {
            A = 1,
        }
    );
    assert!(
        crate::code::expand(input)
            .unwrap_err()
            .to_string()
            .contains("fixed-width")
    );
    let input = parse_quote!(
        #[repr(u8)]
        enum Bad {
            A = 1,
            B,
        }
    );
    assert!(
        crate::code::expand(input)
            .unwrap_err()
            .to_string()
            .contains("explicit")
    );
}

#[test]
fn frame_reports_ambiguous_variable_length_rules() {
    let input = parse_quote! {
        struct Bad {
            #[frame(sequence(prefix = u8, max = 4, count = count, fill = 0))]
            values: [u16; 4],
        }
    };
    assert!(
        crate::frame::expand(input)
            .unwrap_err()
            .to_string()
            .contains("cannot be mixed")
    );
    let input = parse_quote! {
        struct Bad {
            #[frame(sequence(count = count, fill = 0))]
            values: [u16; 4],
            count: u8,
        }
    };
    assert!(
        crate::frame::expand(input)
            .unwrap_err()
            .to_string()
            .contains("earlier field")
    );
}

#[test]
fn frame_checks_field_shapes_and_repeated_options() {
    let input = parse_quote! { struct Bad { #[frame(bytes(prefix = u8, max = 4))] value: u32 } };
    assert!(
        crate::frame::expand(input)
            .unwrap_err()
            .to_string()
            .contains("Vec")
    );
    let input =
        parse_quote! { struct Bad { #[frame(text(prefix = u8, max = 4, max = 5))] value: String } };
    assert!(
        crate::frame::expand(input)
            .unwrap_err()
            .to_string()
            .contains("repeated")
    );
    let input = parse_quote! { struct Bad { #[frame(code, code)] value: u8 } };
    assert!(
        crate::frame::expand(input)
            .unwrap_err()
            .to_string()
            .contains("one frame")
    );
}

#[test]
fn contract_requires_both_types_and_rejects_duplicates() {
    let input = parse_quote!(
        #[contract(request = A)]
        struct Bad;
    );
    assert!(
        crate::contract::expand(input)
            .unwrap_err()
            .to_string()
            .contains("response")
    );
    let input = parse_quote!(
        #[contract(request = A, request = B, response = C)]
        struct Bad;
    );
    assert!(
        crate::contract::expand(input)
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
}

#[test]
fn interface_validates_tags_operations_and_names() {
    use quote::quote;
    let cases = [
        (
            quote!(tag = i8),
            quote!(
                mod api {}
            ),
            "unsigned",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {}
            ),
            "at least one",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api;
            ),
            "inline",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    #[operation(code = 256, request = (), response = ())]
                    struct A;
                }
            ),
            "range",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = ())]
                    struct A;
                    #[operation(code = 1, request = (), response = ())]
                    struct B;
                }
            ),
            "duplicate operation code",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    #[operation(code = 1, request = ())]
                    struct A;
                }
            ),
            "response",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = ())]
                    struct A<T>(T);
                }
            ),
            "unit struct",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    struct Request;
                }
            ),
            "reserved",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    use super::Foo as Response;
                }
            ),
            "reserved",
        ),
        (
            quote!(tag = u8, dispatch),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = ())]
                    struct A;
                }
            ),
            "handler",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = (), handler = a)]
                    struct A;
                }
            ),
            "handler",
        ),
        (
            quote!(tag = u8, dispatch),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = (), handler = a)]
                    struct A;
                    #[operation(code = 2, request = (), response = (), handler = a)]
                    struct B;
                }
            ),
            "duplicate handler",
        ),
    ];
    for (attributes, input, expected) in cases {
        let error = crate::interface::expand(attributes, input).unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "expected {expected}, got {error}"
        );
    }
}

#[test]
fn interface_validates_client_options_and_method_collisions() {
    use quote::quote;
    let cases = [
        (
            quote!(tag = u8, client),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = ())]
                    struct A;
                }
            ),
            "call is required",
        ),
        (
            quote!(tag = u8),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = (), call = a)]
                    struct A;
                }
            ),
            "call is required",
        ),
        (
            quote!(tag = u8, client, client),
            quote!(
                mod api {}
            ),
            "optional client",
        ),
        (
            quote!(tag = u8, client),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = (), call = a, call = b)]
                    struct A;
                }
            ),
            "repeated",
        ),
        (
            quote!(tag = u8, client),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = (), call = read)]
                    struct A;
                    #[cfg(any())]
                    #[operation(code = 2, request = (), response = (), call = r#read)]
                    struct B;
                }
            ),
            "duplicate call",
        ),
        (
            quote!(tag = u8, client),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = (), call = r#new)]
                    struct A;
                }
            ),
            "reserved",
        ),
        (
            quote!(tag = u8, client),
            quote!(
                mod api {
                    use super::Something as Client;
                }
            ),
            "reserved",
        ),
    ];
    for (attributes, input, expected) in cases {
        let error = crate::interface::expand(attributes, input).unwrap_err();
        assert!(
            error.to_string().contains(expected),
            "expected {expected}, got {error}"
        );
    }
}

#[test]
fn interface_server_requires_handlers_and_reserves_its_wrapper_name() {
    use quote::quote;
    for (attr, input, expected) in [
        (
            quote!(tag = u8, server),
            quote!(
                mod api {
                    #[operation(code = 1, request = (), response = ())]
                    struct A;
                }
            ),
            "handler",
        ),
        (
            quote!(tag = u8, server, server),
            quote!(
                mod api {}
            ),
            "optional client",
        ),
        (
            quote!(tag = u8, server),
            quote!(
                mod api {
                    struct Server;
                }
            ),
            "reserved",
        ),
    ] {
        let error = crate::interface::expand(attr, input).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    crate::interface::expand(
        quote!(tag = u8, server, dispatch),
        quote!(
            mod api {
                #[operation(code = 1, request = (), response = (), handler = a)]
                struct A;
            }
        ),
    )
    .unwrap();
}
