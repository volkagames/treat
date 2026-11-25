//! `call` and `call_http` read a refusal only up to `RpcClient::REFUSAL_LIMIT`,
//! as the stream methods do, while a success is read whole.
//!
//! The server is a plain socket writing HTTP/1.1 by hand, so an answer can
//! promise more bytes than it sends and then hold the connection open: a
//! client reading such a body to its end would wait forever, which the
//! timeouts below turn into a failure. Needs only `rpc-client`: the bound is
//! the client's own, not the `stream` feature's.
#![cfg(feature = "rpc-client")]

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;
use treat::rpc::client::{ClientError, ClientResult, NoInput, RpcClient};
use treat::rpc::{HttpBodyKind, HttpSpec};

#[derive(Debug, Clone, Copy, PartialEq)]
enum Codes {}

type Called<T> = ClientResult<T, Codes>;

/// What one connection is answered with: a status line and extra headers,
/// and a body that either ends or is cut short of its promised length and
/// then held open.
#[derive(Clone)]
struct Answer {
    status: &'static str,
    headers: &'static [&'static str],
    body: Vec<u8>,
    /// Whether the connection stays open after `body`, short of the length
    /// the headers promise.
    held: bool,
}

/// An answer promising twice the refusal limit, sending a little more than
/// the limit, and then nothing.
fn endless(status: &'static str, headers: &'static [&'static str]) -> Answer {
    Answer {
        status,
        headers,
        body: vec![b'x'; RpcClient::REFUSAL_LIMIT + 4096],
        held: true,
    }
}

/// Answer every connection to a fresh local port with `answer`.
fn serve(answer: Answer) -> RpcClient {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a free local port");
    let address = listener.local_addr().expect("a bound address");
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else {
                continue;
            };
            let answer = answer.clone();
            thread::spawn(move || respond(stream, &answer));
        }
    });
    RpcClient::new(format!("http://{address}"))
}

fn respond(mut stream: TcpStream, answer: &Answer) {
    // The request is small; one read takes it, and its content does not
    // matter to the answer.
    let mut request = [0_u8; 64 * 1024];
    let read = stream.read(&mut request);
    if read.is_err() {
        return;
    }
    let promised = match answer.held {
        true => answer.body.len() * 2,
        false => answer.body.len(),
    };
    let mut head = format!("HTTP/1.1 {}\r\ncontent-length: {promised}\r\n", answer.status);
    for header in answer.headers {
        head.push_str(header);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let written = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(&answer.body));
    if written.is_err() || !answer.held {
        return;
    }
    // Hold the connection open, short of the promised length, for longer than
    // any test here waits.
    thread::sleep(Duration::from_secs(120));
}

/// `future`, or a panic if it does not finish within ten seconds: the
/// client read on past the limit, waiting for bytes that never come.
async fn within<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("returned without reading the body to its end")
}

#[tokio::test]
async fn a_non_2xx_rpc_answer_is_read_only_up_to_the_limit() {
    let client = serve(endless("502 Bad Gateway", &["content-type: application/json"]));
    let called: Called<serde_json::Value> = within(client.call("/rpc/endless", &NoInput)).await;
    assert!(matches!(called, Err(ClientError::Status(502))), "{called:?}");
}

#[tokio::test]
async fn an_answer_declared_a_failure_is_read_only_up_to_the_limit() {
    let client = serve(endless(
        "200 OK",
        &["content-type: application/json", "x-rpc-status: error"],
    ));
    let called: Called<serde_json::Value> = within(client.call("/rpc/endless", &NoInput)).await;
    assert!(matches!(called, Err(ClientError::Decode(_))), "{called:?}");
}

#[tokio::test]
async fn a_non_2xx_http_answer_is_read_only_up_to_the_limit() {
    let client = serve(endless("404 Not Found", &["content-type: text/html"]));
    for (spec, envelope) in [
        (HttpSpec::get("/endless").bare(), false),
        (HttpSpec::get("/endless"), true),
        (HttpSpec::post("/endless").body(HttpBodyKind::Form), true),
    ] {
        let called: Called<serde_json::Value> = within(client.call_http(&spec, &NoInput)).await;
        assert!(
            matches!(called, Err(ClientError::Status(404))),
            "envelope {envelope}: {called:?}"
        );
    }
}

#[tokio::test]
async fn a_small_refusal_is_still_read_from_errors() {
    let client = serve(Answer {
        status: "503 Service Unavailable",
        headers: &["content-type: application/json"],
        body: br#"{"errors":[{"code":"busy"}]}"#.to_vec(),
        held: false,
    });
    let called: Called<serde_json::Value> = within(client.call("/rpc/busy", &NoInput)).await;
    let codes: Vec<String> = match &called {
        Err(ClientError::Api(errors)) => errors.messages.iter().map(|e| e.code.clone()).collect(),
        _ => Vec::new(),
    };
    assert_eq!(codes, ["busy"], "{called:?}");
}

#[tokio::test]
async fn a_success_past_the_limit_is_read_whole() {
    let text = "y".repeat(RpcClient::REFUSAL_LIMIT * 2);
    let body = serde_json::to_vec(&serde_json::json!({ "data": text })).expect("serializes");
    let client = serve(Answer {
        status: "200 OK",
        headers: &["content-type: application/json"],
        body,
        held: false,
    });
    let called: Called<String> = within(client.call("/rpc/large", &NoInput)).await;
    assert_eq!(called.ok().map(|data| data.len()), Some(text.len()));

    let bare: Called<serde_json::Value> = within(client.call_http(&HttpSpec::get("/large").bare(), &NoInput)).await;
    assert_eq!(
        bare.ok()
            .and_then(|value| value.get("data").and_then(|data| data.as_str().map(str::len))),
        Some(text.len())
    );
}
