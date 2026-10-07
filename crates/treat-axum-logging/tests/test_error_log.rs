//! Coverage for `error_log`: the level follows the error's source chain, and
//! hops recorded by `track()` are logged without counting as a cause.

use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::SubscriberExt;
use treat_axum_logging::error_log;
use treat_core::error;

/// Records each event as `LEVEL field=value ...`.
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
        let mut visitor = Visitor(vec![event.metadata().level().to_string()]);
        event.record(&mut visitor);
        self.0.lock().expect("capture lock").push(visitor.0.join(" "));
    }
}

fn capture(f: impl FnOnce()) -> String {
    let events = Capture::default();
    let subscriber = tracing_subscriber::registry().with(events.clone());
    tracing::subscriber::with_default(subscriber, f);
    events.0.lock().expect("capture lock").join(" | ")
}

// A tracked business error has no cause, so it stays at `debug!` (#1).
#[test]
fn a_tracked_business_error_logs_at_debug() {
    let hop = line!() + 1;
    let e = error("user_not_found").with_message("no such user").track();
    let logs = capture(|| error_log(&e));

    assert!(logs.starts_with("DEBUG "), "{logs}");
    assert!(logs.contains(&format!("error_hops=\"{}:{hop}:", file!())), "{logs}");
    assert!(logs.contains("message=no such user"), "{logs}");
}

// A tracked error with a cause keeps the cause in its message (#1).
#[test]
fn a_tracked_error_with_a_cause_logs_at_error() {
    let e = error("ticket_send_error")
        .with_message("unable send ticket")
        .with_error(erris::report!("unable create new ticket"))
        .track();
    let logs = capture(|| error_log(&e));

    assert!(logs.starts_with("ERROR "), "{logs}");
    assert!(
        logs.contains("message=unable send ticket, unable create new ticket"),
        "{logs}"
    );
}

#[test]
fn an_untracked_error_has_no_hops_field() {
    let e = error("user_not_found");
    let logs = capture(|| error_log(&e));
    assert!(!logs.contains("error_hops"), "{logs}");
}
