//! The derives' own diagnostics: input a derive must refuse with a pointed
//! error instead of generating code that fails somewhere inside the expansion.

use syn::DeriveInput;

/// The messages of every error `result` carries, combined ones included.
fn messages(result: syn::Result<proc_macro2::TokenStream>) -> Vec<String> {
    match result {
        Ok(tokens) => panic!("expected the derive to refuse, it generated: {tokens}"),
        Err(error) => error.into_iter().map(|error| error.to_string()).collect(),
    }
}

#[test]
fn api_error_names_every_variant_without_a_code() {
    let input: DeriveInput = syn::parse_quote! {
        enum ServiceError {
            #[code("forbidden")]
            Forbidden,
            Missing,
            AlsoMissing(u8),
        }
    };
    let messages = messages(crate::api_error::derive(&input));
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert!(messages[0].contains("`Missing` is missing a #[code"), "{messages:?}");
    assert!(
        messages[1].contains("`AlsoMissing` is missing a #[code"),
        "{messages:?}"
    );
}

#[test]
fn api_error_catch_all_must_hold_exactly_one_field() {
    let input: DeriveInput = syn::parse_quote! {
        enum ServiceError {
            #[catch_all]
            #[code("internal")]
            Internal(Report, u8),
        }
    };
    let messages = messages(crate::api_error::derive(&input));
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert!(messages[0].contains("exactly one field"), "{messages:?}");
}

#[test]
fn api_error_catch_all_may_appear_at_most_once() {
    let input: DeriveInput = syn::parse_quote! {
        enum ServiceError {
            #[catch_all]
            #[code("internal")]
            Internal(Report),
            #[catch_all]
            #[code("other")]
            Other(Report),
        }
    };
    let messages = messages(crate::api_error::derive(&input));
    assert!(messages.iter().all(|m| m.contains("at most once")), "{messages:?}");

    // Zero is fine: the catch-all is optional.
    let input: DeriveInput = syn::parse_quote! {
        enum ServiceError {
            #[code("forbidden")]
            Forbidden,
        }
    };
    assert!(crate::api_error::derive(&input).is_ok());
}

#[test]
fn from_error_message_refuses_a_code_claimed_twice() {
    let input: DeriveInput = syn::parse_quote! {
        enum ClientError {
            #[code("gone")]
            Gone(ErrorMessage),
            #[code("gone")]
            AlsoGone(ErrorMessage),
            #[code("gone")]
            GoneAgain(ErrorMessage),
            #[code("_")]
            Other(ErrorMessage),
        }
    };
    let messages = messages(crate::from_error_message::impl_from_error_message(&input));
    // Each duplicate is reported, each paired with the variant it collides with.
    assert_eq!(messages.len(), 4, "{messages:?}");
    assert!(messages[0].contains("already used by `Gone`"), "{messages:?}");
    assert!(messages[1].contains("`Gone` declares"), "{messages:?}");
    assert!(messages[2].contains("already used by `Gone`"), "{messages:?}");
}

#[test]
fn generic_enums_carry_their_generics_into_every_impl() {
    let input: DeriveInput = syn::parse_quote! {
        enum Code<T: Clone> {
            #[message("bad {arg_0}")]
            Bad(T),
        }
    };
    let tokens = crate::api_error_code::derive(&input)
        .expect("generic ApiErrorCode")
        .to_string();
    assert!(tokens.contains("for Code < T >"), "{tokens}");

    let input: DeriveInput = syn::parse_quote! {
        enum ServiceError<T> {
            #[code("bad")]
            Bad(T),
        }
    };
    let tokens = crate::api_error::derive(&input).expect("generic ApiError").to_string();
    assert!(tokens.contains("From < ServiceError < T > >"), "{tokens}");

    let input: DeriveInput = syn::parse_quote! {
        enum ClientError<E> {
            #[code("_")]
            Other(E),
        }
    };
    let tokens = crate::from_error_message::impl_from_error_message(&input)
        .expect("generic FromErrorMessage")
        .to_string();
    assert!(tokens.contains("for ClientError < E >"), "{tokens}");
}
