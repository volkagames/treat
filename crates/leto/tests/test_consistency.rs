//! The actix and axum adapters must serialize an `ApiError` to byte-identical
//! JSON, since both go through the same `into_api_response` builder.
#![cfg(all(feature = "actix", feature = "axum"))]

use leto::{ApiResponse, error};

#[tokio::test]
async fn actix_and_axum_emit_the_same_envelope() {
    use actix_web::ResponseError;
    use actix_web::body::MessageBody;
    use axum::response::IntoResponse;

    let actix_bytes = error("boom")
        .with_message("bad")
        .error_response()
        .into_body()
        .try_into_bytes()
        .expect("actix body");
    let actix: ApiResponse = serde_json::from_slice(&actix_bytes).expect("actix json");

    let axum_resp = error("boom").with_message("bad").into_response();
    let axum_bytes = axum::body::to_bytes(axum_resp.into_body(), usize::MAX)
        .await
        .expect("axum body");
    let axum: ApiResponse = serde_json::from_slice(&axum_bytes).expect("axum json");

    assert_eq!(actix, axum);
    assert_eq!(actix.first_error_code(), Some("boom"));
}

#[test]
fn actix_and_axum_return_the_same_status() {
    use actix_web::ResponseError;
    use axum::response::IntoResponse;

    let actix_status = error("not_found").with_status(404).status_code().as_u16();
    let axum_status = error("not_found").with_status(404).into_response().status().as_u16();
    assert_eq!(actix_status, 404);
    assert_eq!(actix_status, axum_status);
}
