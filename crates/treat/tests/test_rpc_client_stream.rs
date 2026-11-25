//! `treat::rpc::client` with `rpc-stream`: the types a subscription's caller
//! names. The SSE framing itself is covered by `treat-rpc`'s unit tests, since
//! the parser is not public, and a subscription over a socket by `test_rpc_e2e`.
#![cfg(all(feature = "rpc-client", feature = "rpc-stream"))]

use treat::rpc::client::{EventStream, StreamError};
use treat::rpc::futures_util::StreamExt;

#[test]
fn stream_error_display_names_the_failure() {
    assert_eq!(
        StreamError::Transport("reset".into()).to_string(),
        "stream transport error: reset"
    );
    assert_eq!(
        StreamError::Decode("expected u32".into()).to_string(),
        "could not decode event: expected u32"
    );
    let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(StreamError::Decode("x".into()));
    assert_eq!(boxed.to_string(), "could not decode event: x");
}

#[test]
fn the_sse_buffer_limit_is_sixteen_mebibytes() {
    assert_eq!(treat::rpc::client::RpcClient::SSE_BUFFER_LIMIT, 16 << 20);
}

#[tokio::test]
async fn event_stream_is_a_boxed_stream_of_results() {
    // What a test double of a generated subscription returns.
    let events: EventStream<u32> = Box::pin(treat::rpc::futures_util::stream::iter([
        Ok(1),
        Err(StreamError::Decode("bad".into())),
    ]));
    let items: Vec<Result<u32, StreamError>> = events.collect().await;
    assert!(matches!(items.as_slice(), [Ok(1), Err(StreamError::Decode(_))]));
}

#[test]
fn stream_futures_are_send() {
    // A generated method is an `async fn` over these; a caller spawns it.
    fn assert_send<T: Send>(_: T) {}
    let client = treat::rpc::client::RpcClient::new("http://localhost");
    let input = treat::rpc::client::NoInput;
    assert_send(client.subscribe::<_, u32, ()>("/watch", &[], &input));
    assert_send(client.raw::<_, ()>("/report", &input));
    assert_send(client.raw_request::<_, ()>("/convert", &[], &input, vec![1u8], "text/csv"));
    assert_send(client.raw_request_json::<_, u32, ()>("/upload", &[], &input, vec![1u8], "text/csv"));
}
