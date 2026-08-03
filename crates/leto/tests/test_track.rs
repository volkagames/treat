//! Coverage for `ApiError::track` and the `ApiErrorTrack::track_api_error`
//! extension. Both preludes are imported to assert the trait sets don't clash.
#![allow(unused_imports)]

use erris::prelude::*;
use leto::error;
use leto::prelude::*;

#[test]
fn track_adds_a_source_frame() {
    let e = error("e");
    assert!(e.source().is_none());

    let tracked = e.track();
    assert!(tracked.source().is_some());
    assert_eq!(*tracked.code(), "e");
}

#[test]
fn track_api_error_passes_ok_through() {
    let ok: Result<ApiResponse<()>, leto::ApiError> = Ok(success(()));
    assert!(ok.track_api_error().is_ok());
}

#[test]
fn track_api_error_tracks_the_err() {
    let err: Result<ApiResponse<()>, leto::ApiError> = Err(error("e"));
    let tracked = err.track_api_error().expect_err("expected error");
    assert!(tracked.source().is_some());
    assert_eq!(*tracked.code(), "e");
}
