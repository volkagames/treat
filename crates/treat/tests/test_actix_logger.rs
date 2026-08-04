//! Coverage for the actix error loggers.
//!
//! `Error::as_error::<T>` is a `TypeId` check, so a logger can only name a
//! *concrete* `ApiError<C>`. The plain [`error_log`] names the default
//! `ApiError<&'static str>`; a service using a typed code enum needs
//! [`error_log_for`] to keep its structured fields, otherwise the error falls
//! into the generic arm and `error_code` disappears from the logs.
#![cfg(feature = "actix")]

use treat::error;
use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;

#[derive(Clone, Debug, PartialEq)]
enum MyCode {
    Boom,
}

impl std::fmt::Display for MyCode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "my.boom")
    }
}

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Capture {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Visitor(Vec<String>);
        impl tracing::field::Visit for Visitor {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push(format!("{}={:?}", field.name(), value));
            }
        }
        let mut visitor = Visitor(vec![]);
        event.record(&mut visitor);
        self.0.lock().expect("capture lock").push(visitor.0.join(" "));
    }
}

/// Run `f` with a capturing subscriber and return the recorded events.
fn capture(f: impl FnOnce()) -> String {
    let events = Capture::default();
    let subscriber = tracing_subscriber::registry().with(events.clone());
    tracing::subscriber::with_default(subscriber, f);
    events.0.lock().expect("capture lock").join(" | ")
}

#[test]
fn typed_code_logger_emits_structured_fields() {
    let err: actix_web::Error = error(MyCode::Boom).with_message("m").into();
    let logs = capture(|| treat::error_log_for::<MyCode>(&err));
    assert!(logs.contains("error_code"), "no structured code: {logs}");
    assert!(logs.contains("my.boom"), "typed code missing: {logs}");
}

/// A typed logger must still recognise the default code, which library helpers
/// commonly raise — a mixed codebase keeps structured logs for both.
#[test]
fn typed_code_logger_falls_back_to_the_default_code() {
    let err: actix_web::Error = error("from_lib").with_message("m").into();
    let logs = capture(|| treat::error_log_for::<MyCode>(&err));
    assert!(logs.contains("error_code"), "lost structure: {logs}");
    assert!(logs.contains("from_lib"), "default code missing: {logs}");
}

#[test]
fn default_logger_emits_structured_fields_for_the_default_code() {
    let err: actix_web::Error = error("plain").with_message("m").into();
    let logs = capture(|| treat::error_log(&err));
    assert!(logs.contains("error_code"), "no structured code: {logs}");
    assert!(logs.contains("plain"), "code missing: {logs}");
}

#[test]
fn debug_logger_mirrors_the_error_logger() {
    let err: actix_web::Error = error(MyCode::Boom).with_message("m").into();
    let logs = capture(|| treat::debug_log_for::<MyCode>(&err));
    assert!(logs.contains("my.boom"), "typed code missing: {logs}");
}

/// A non-`treat` error has no code to report and keeps the generic arm.
#[test]
fn foreign_error_uses_the_generic_arm() {
    let err = actix_web::error::ErrorBadRequest("nope");
    let logs = capture(|| treat::error_log_for::<MyCode>(&err));
    assert!(logs.contains("request error"), "expected the generic arm: {logs}");
    assert!(!logs.contains("error_code"), "unexpected structured code: {logs}");
}
