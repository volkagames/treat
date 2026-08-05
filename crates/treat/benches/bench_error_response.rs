//! Cost of building an error response, and specifically of stashing the
//! type-erased error in the response extensions.
//!
//! The adapters take `&self`, so the stash clones the `ApiError`. This measures
//! whether that clone is material next to the JSON serialization that dominates
//! the same code path.

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use std::sync::Arc;
use treat::{ApiError, ApiErrorHandler, error};

/// A realistic error: message, locator, explicit status, and a cause chain.
fn realistic() -> ApiError {
    error("user_not_found")
        .with_message("no such user")
        .with_status(404)
        .with_pointer("/data/attributes/id")
        .with_error(erris::report!("row missing"))
}

/// The clone + `Arc` the stash performs, in isolation.
fn bench_stash_only(c: &mut Criterion) {
    let err = realistic();
    c.bench_function("stash: clone + Arc<dyn ApiErrorHandler>", |b| {
        b.iter(|| {
            let stashed = Arc::new(black_box(&err).clone()) as Arc<dyn ApiErrorHandler>;
            black_box(&stashed);
        });
    });
}

/// The envelope build + JSON serialization the same path already does.
fn bench_serialize_only(c: &mut Criterion) {
    let err = realistic();
    c.bench_function("body: into_api_response + serde_json", |b| {
        b.iter(|| {
            let envelope = black_box(&err).into_api_response::<()>();
            let json = serde_json::to_vec(&envelope).expect("serialize");
            black_box(json);
        });
    });
}

#[cfg(feature = "actix")]
fn bench_actix_error_response(c: &mut Criterion) {
    use actix_web::ResponseError;

    let err = realistic();
    c.bench_function("actix: error_response (with stash)", |b| {
        b.iter(|| {
            let response = black_box(&err).error_response();
            black_box(response);
        });
    });
}

#[cfg(feature = "axum")]
fn bench_axum_into_response(c: &mut Criterion) {
    use axum::response::IntoResponse;

    let err = realistic();
    c.bench_function("axum: into_response (with stash)", |b| {
        b.iter(|| {
            let response = black_box(&err).clone().into_response();
            black_box(response);
        });
    });
}

#[cfg(feature = "poem")]
fn bench_poem_as_response(c: &mut Criterion) {
    use poem::error::ResponseError;

    let err = realistic();
    c.bench_function("poem: as_response (with stash)", |b| {
        b.iter(|| {
            let response = black_box(&err).as_response();
            black_box(response);
        });
    });
}

criterion_group!(benches, bench_stash_only, bench_serialize_only);

#[cfg(feature = "actix")]
criterion_group!(actix_benches, bench_actix_error_response);
#[cfg(feature = "axum")]
criterion_group!(axum_benches, bench_axum_into_response);
#[cfg(feature = "poem")]
criterion_group!(poem_benches, bench_poem_as_response);

#[cfg(all(feature = "actix", feature = "axum", feature = "poem"))]
criterion_main!(benches, actix_benches, axum_benches, poem_benches);
#[cfg(not(all(feature = "actix", feature = "axum", feature = "poem")))]
criterion_main!(benches);
