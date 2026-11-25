//! Regressions for the actix observability middleware: `x-request-id` and
//! logging of a rejection from an inner middleware, the `otel.status_code` rule, `root_span!`
//! outside the `Logger`, a custom builder with a custom error logger, nested
//! loggers, and the extractor rejections.
#![cfg(feature = "actix-middleware")]

use actix_web::body::BoxBody;
use actix_web::dev::{Service, ServiceRequest, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::{App, HttpResponse, test as atest, web};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::subscriber::Subscriber;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use treat::{DefaultRootSpanBuilder, Logger, RequestId, RootSpanBuilder, X_REQUEST_ID};

/// `(field, value)` for every field set on a span named `HTTP request`.
#[derive(Clone, Default)]
struct RootFields(Arc<Mutex<Vec<(String, String)>>>);

impl RootFields {
    fn get(&self, name: &str) -> Option<String> {
        let fields = self.0.lock().expect("capture poisoned");
        fields
            .iter()
            .rev()
            .find(|(field, _)| field == name)
            .map(|(_, value)| value.clone())
    }

    fn clear(&self) {
        self.0.lock().expect("capture poisoned").clear();
    }
}

impl Visit for RootFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0
            .lock()
            .expect("capture poisoned")
            .push((field.name().to_owned(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.0
            .lock()
            .expect("capture poisoned")
            .push((field.name().to_owned(), value.to_owned()));
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> tracing_subscriber::Layer<S> for RootFields {
    fn on_new_span(&self, attrs: &tracing::span::Attributes<'_>, _id: &tracing::Id, _ctx: Context<'_, S>) {
        if attrs.metadata().name() == "HTTP request" {
            attrs.record(&mut self.clone());
        }
    }

    fn on_record(&self, id: &tracing::Id, values: &tracing::span::Record<'_>, ctx: Context<'_, S>) {
        let is_root = ctx.span(id).is_some_and(|span| span.name() == "HTTP request");
        if is_root {
            values.record(&mut self.clone());
        }
    }
}

fn install() -> (RootFields, tracing::subscriber::DefaultGuard) {
    let fields = RootFields::default();
    let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(fields.clone()));
    (fields, guard)
}

fn reject_unauthorized(
    _: ServiceRequest,
    _: &(),
) -> std::future::Ready<Result<ServiceResponse<BoxBody>, actix_web::Error>> {
    std::future::ready(Err(actix_web::error::ErrorUnauthorized("no token")))
}

/// An inner middleware that rejects with `Err` has no response for the logger
/// to stamp: the error is logged and passed on as is, so its type is still
/// the one the middleware returned.
#[actix_web::test]
async fn an_err_from_an_inner_middleware_is_logged_and_passed_on_as_is() {
    let logged = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = logged.clone();
    let logger = Logger::with_error_logger(move |err: &actix_web::Error| {
        sink.lock().expect("log poisoned").push(err.to_string());
    });
    let app = atest::init_service(
        App::new()
            .wrap_fn(|req, _srv| reject_unauthorized(req, &()))
            .wrap(logger)
            .route("/", web::get().to(HttpResponse::Ok)),
    )
    .await;

    let outcome = app.call(atest::TestRequest::get().uri("/").to_request()).await;
    let err = match outcome {
        Ok(_) => panic!("the inner middleware rejects the request"),
        Err(err) => err,
    };
    assert_eq!(err.as_response_error().status_code(), StatusCode::UNAUTHORIZED);
    assert_eq!(*logged.lock().expect("log poisoned"), vec!["no token".to_owned()]);
}

/// An inner middleware that answers `Ok(req.error_response(err))` gets the
/// id on its response, and an `ApiError` keeps its envelope, status and the
/// type-erased error in the response extensions.
#[actix_web::test]
async fn a_rejection_answered_as_a_response_carries_the_request_id() {
    let app = atest::init_service(
        App::new()
            .wrap_fn(|req, _srv| std::future::ready(Ok(req.error_response(treat::error("denied").with_status(403)))))
            .wrap(Logger::default())
            .route("/", web::get().to(HttpResponse::Ok)),
    )
    .await;

    let supplied = "67e55044-10b1-426f-9247-bb680e5fe0c8";
    let request = atest::TestRequest::get()
        .uri("/")
        .insert_header((X_REQUEST_ID, supplied))
        .to_request();
    let response = atest::call_service(&app, request).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers().get(X_REQUEST_ID).and_then(|v| v.to_str().ok()),
        Some(supplied)
    );
    let api_error = treat::response_get_api_error_actix(response.response()).expect("error in the extensions");
    assert_eq!(api_error.code(), "denied");
}

/// A handler-set header still wins on a successful response.
#[actix_web::test]
async fn a_handler_set_header_wins() {
    let app = atest::init_service(App::new().wrap(Logger::default()).route(
        "/",
        web::get().to(|| async { HttpResponse::Ok().insert_header((X_REQUEST_ID, "mine")).finish() }),
    ))
    .await;
    let response = atest::call_service(&app, atest::TestRequest::get().uri("/").to_request()).await;
    assert_eq!(
        response.headers().get(X_REQUEST_ID).and_then(|v| v.to_str().ok()),
        Some("mine")
    );
}

async fn api(status: web::Path<u16>) -> Result<HttpResponse, treat::ApiError> {
    Err(treat::error("boom").with_message("m").with_status(*status))
}

async fn plain(status: web::Path<u16>) -> HttpResponse {
    HttpResponse::build(StatusCode::from_u16(*status).expect("status")).finish()
}

/// `otel.status_code` is `ERROR` iff the status the client gets is a 5xx,
/// whether or not the response carries an error (the tower layer's rule).
#[actix_web::test]
async fn otel_status_is_error_iff_the_status_is_5xx() {
    let (fields, _guard) = install();
    let app = atest::init_service(
        App::new()
            .wrap(Logger::default())
            .route("/api/{status}", web::get().to(api))
            .route("/plain/{status}", web::get().to(plain)),
    )
    .await;

    for (uri, status, otel, exception) in [
        ("/plain/200", 200, "OK", false),
        ("/plain/404", 404, "OK", false),
        // Was `OK`: a response without an attached error never got `ERROR`.
        ("/plain/503", 503, "ERROR", false),
        // Was `ERROR`: a handled `ApiError` at 200 is not a server failure.
        ("/api/200", 200, "OK", true),
        ("/api/404", 404, "OK", true),
        ("/api/500", 500, "ERROR", true),
    ] {
        fields.clear();
        let response = atest::call_service(&app, atest::TestRequest::get().uri(uri).to_request()).await;
        assert_eq!(response.status().as_u16(), status, "{uri}");
        assert_eq!(fields.get("otel.status_code").as_deref(), Some(otel), "{uri}");
        assert_eq!(fields.get("http.status_code"), Some(status.to_string()), "{uri}");
        let message = fields.get("exception.message");
        if exception {
            assert_eq!(message.as_deref(), Some("treat error: boom, message: m"), "{uri}");
        } else {
            assert_eq!(message, None, "{uri}");
        }
    }
}

/// `root_span!` outside the `Logger` (no `RequestId` in the extensions) used
/// to panic; it now generates an id for the span.
#[test]
fn root_span_without_the_logger_generates_a_request_id() {
    let (fields, _guard) = install();
    let request = atest::TestRequest::get().uri("/x").to_srv_request();
    let _span = DefaultRootSpanBuilder::on_request_start(&request);
    let request_id = fields.get("request_id").expect("request_id recorded");
    assert!(uuid_like(&request_id), "{request_id}");
}

fn uuid_like(value: &str) -> bool {
    value.len() == 36 && value.chars().filter(|c| *c == '-').count() == 4
}

struct Tenanted;

impl RootSpanBuilder for Tenanted {
    fn on_request_start(request: &ServiceRequest) -> tracing::Span {
        treat::root_span!(request, tenant = "acme")
    }

    fn on_request_end<B>(span: tracing::Span, outcome: &Result<ServiceResponse<B>, actix_web::Error>) {
        DefaultRootSpanBuilder::on_request_end(span, outcome)
    }
}

static TYPED_LOGGER_CALLS: Mutex<u32> = Mutex::new(0);

fn count_errors(_: &actix_web::Error) {
    *TYPED_LOGGER_CALLS.lock().expect("counter poisoned") += 1;
}

/// A custom root span builder and a custom error logger combine.
#[actix_web::test]
async fn a_custom_builder_takes_a_custom_error_logger() {
    let (fields, _guard) = install();
    let app = atest::init_service(
        App::new()
            .wrap(Logger::<Tenanted>::new().error_logger(count_errors))
            .route("/api/{status}", web::get().to(api)),
    )
    .await;
    let response = atest::call_service(&app, atest::TestRequest::get().uri("/api/500").to_request()).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(response.headers().contains_key(X_REQUEST_ID));
    assert_eq!(*TYPED_LOGGER_CALLS.lock().expect("counter poisoned"), 1);
    assert_eq!(fields.get("tenant").as_deref(), Some("acme"));
    assert_eq!(fields.get("otel.status_code").as_deref(), Some("ERROR"));
}

/// Without the `Logger` in front, the `RequestId` / `RootSpan` extractors
/// reject with a `treat` envelope, not a text/plain body.
#[actix_web::test]
async fn extractor_rejections_render_the_envelope() {
    async fn wants_id(_: RequestId) -> HttpResponse {
        HttpResponse::Ok().finish()
    }
    async fn wants_span(_: treat::RootSpan) -> HttpResponse {
        HttpResponse::Ok().finish()
    }
    let app = atest::init_service(
        App::new()
            .route("/id", web::get().to(wants_id))
            .route("/span", web::get().to(wants_span)),
    )
    .await;

    for (uri, code) in [("/id", "request_id_unavailable"), ("/span", "root_span_unavailable")] {
        let response = atest::call_service(&app, atest::TestRequest::get().uri(uri).to_request()).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR, "{uri}");
        let body: serde_json::Value = atest::read_body_json(response).await;
        assert_eq!(body["errors"][0]["code"], code, "{uri}: {body}");
    }
}

/// With `actix-telemetry` but no OpenTelemetry layer on the subscriber, the
/// span has no trace; `trace_id` stays empty instead of 32 zeros.
#[cfg(feature = "actix-telemetry")]
#[test]
fn no_otel_layer_leaves_trace_id_empty() {
    let (fields, _guard) = install();
    let request = atest::TestRequest::get().uri("/x").to_srv_request();
    let _span = DefaultRootSpanBuilder::on_request_start(&request);
    assert_eq!(fields.get("trace_id"), None);
}

/// `error_code` of every event logged while the capture is installed (`None`
/// for an event without one, e.g. the generic `request error` arm).
#[derive(Clone, Default)]
struct EventCodes(Arc<Mutex<Vec<Option<String>>>>);

struct ErrorCodeVisitor(Option<String>);

impl Visit for ErrorCodeVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "error_code" {
            self.0 = Some(format!("{value:?}"));
        }
    }
}

impl<S: Subscriber> tracing_subscriber::Layer<S> for EventCodes {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != tracing::Level::ERROR {
            return;
        }
        let mut visitor = ErrorCodeVisitor(None);
        event.record(&mut visitor);
        self.0.lock().expect("capture poisoned").push(visitor.0);
    }
}

/// With two `Logger`s nested (one on the `App`, one on a scope), an inner
/// `Err` rejection reaches both untouched, so both log the `ApiError` with its
/// structured fields.
#[actix_web::test]
async fn nested_loggers_both_log_the_api_error() {
    let codes = EventCodes::default();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(codes.clone()));
    let app = atest::init_service(
        App::new().wrap(Logger::default()).service(
            web::scope("/s")
                .wrap_fn(|_req, _srv| {
                    std::future::ready(Err::<ServiceResponse<BoxBody>, _>(
                        treat::error("denied").with_status(403).into(),
                    ))
                })
                .wrap(Logger::default())
                .route("", web::get().to(HttpResponse::Ok)),
        ),
    )
    .await;

    let outcome = app.call(atest::TestRequest::get().uri("/s").to_request()).await;
    let err = match outcome {
        Ok(_) => panic!("the scope's middleware rejects the request"),
        Err(err) => err,
    };
    assert!(err.as_error::<treat::ApiError>().is_some(), "passed on as is: {err:?}");
    assert_eq!(err.error_response().status(), StatusCode::FORBIDDEN);

    let logged = codes.0.lock().expect("capture poisoned").clone();
    assert_eq!(logged, vec![Some("denied".to_owned()), Some("denied".to_owned())]);
}
