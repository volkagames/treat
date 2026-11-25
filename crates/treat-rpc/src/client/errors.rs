//! What a call fails with: [`ClientError`] and the server's [`ApiErrors`].

use crate::WireEnum;
use std::fmt;
use std::marker::PhantomData;
use treat_core::ErrorMessage;

/// Everything a call can fail with.
///
/// `Api` is the server answering with `errors[]`; the rest never reached the
/// application layer. Only `Api` carries a code the caller can branch on.
///
/// Non-exhaustive: the type is `treat`'s, versioned apart from the schema a
/// client is generated from, so a new way to fail can arrive in a minor
/// release. A `match` names the variants it handles and gives the rest one arm.
#[derive(Debug)]
#[non_exhaustive]
pub enum ClientError<C> {
    /// The server reported one or more errors in the envelope.
    Api(ApiErrors<C>),
    /// The request never completed: connection, TLS or timeout, or the body
    /// broke off while being read.
    Transport(String),
    /// The request could not be built, so nothing was sent: the input did not
    /// serialize, or a path parameter is missing or has no URL spelling.
    Encode(String),
    /// A 2xx body that was neither a usable envelope nor, for an `@http`
    /// operation with `envelope: false`, the value the operation answers with.
    Decode(String),
    /// A non-2xx status with no usable envelope, such as a proxy error page.
    Status(u16),
}

impl<C> fmt::Display for ClientError<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::Api(e) => write!(f, "{e}"),
            ClientError::Transport(m) => write!(f, "transport error: {m}"),
            ClientError::Encode(m) => write!(f, "could not encode request: {m}"),
            ClientError::Decode(m) => write!(f, "could not decode response: {m}"),
            ClientError::Status(s) => write!(f, "request failed with status {s}"),
        }
    }
}

impl<C: fmt::Debug> std::error::Error for ClientError<C> {}

/// The result of a call to an operation whose failures are the code set `C`.
pub type ClientResult<T, C> = Result<T, ClientError<C>>;

/// The errors from one failed call, in the order the server listed them.
///
/// A list, not one entry: validating a whole input reports every problem.
#[derive(Debug, Clone, PartialEq)]
pub struct ApiErrors<C> {
    /// The entries as the server sent them, codes included verbatim.
    pub messages: Vec<ErrorMessage>,
    /// The operation's code set, carried in the type alone.
    code: PhantomData<C>,
}

impl<C> ApiErrors<C> {
    /// The failure the server reported with `messages`.
    ///
    /// The client builds it from an envelope; it is public for a test double
    /// of a generated client, which has to fail the way the real one does.
    pub fn new(messages: Vec<ErrorMessage>) -> Self {
        Self {
            messages,
            code: PhantomData,
        }
    }

    /// The first entry, which is the one to show when only one can be shown.
    pub fn first(&self) -> Option<&ErrorMessage> {
        self.messages.first()
    }

    /// JSON Pointer to the field that caused the failure, when the server
    /// located one — set for `invalid_body` rejections.
    pub fn pointer(&self) -> Option<&str> {
        self.messages.iter().find_map(|e| e.source.as_ref()?.pointer.as_deref())
    }

    /// The first entry's code as a value of `R`, read by wire name.
    ///
    /// `R` is the flat registry, for logging or a `match` over every code the
    /// API has, or the operation's set `C` itself, since a set reads names the
    /// same way. `None` when the server sent a code `R` does not know, which is
    /// what a server newer than the client looks like, or no entry at all.
    pub fn code<R: WireEnum>(&self) -> Option<R> {
        R::from_name(&self.first()?.code)
    }
}

/// Reading a failure against the set the calling operation declares.
///
/// A code the operation cannot return is not a variant of `C`, so asking about
/// it does not compile rather than answering `false` forever. The comparison is
/// by wire name ([`WireEnum::as_str`]), the string the server writes, rather
/// than by `Display`, which a set renamed through serde alone spells otherwise;
/// a generated set derives `WireEnum`.
impl<C: WireEnum> ApiErrors<C> {
    /// Whether any entry carries `code`.
    pub fn is(&self, code: C) -> bool {
        let code = code.as_str();
        self.messages.iter().any(|e| e.code == code)
    }

    /// First `meta` payload reported against `code`.
    pub fn meta_for(&self, code: C) -> Option<&serde_json::Map<String, serde_json::Value>> {
        let code = code.as_str();
        self.messages
            .iter()
            .find(|e| e.code == code)
            .and_then(|e| e.meta.as_ref())
    }
}

impl<C> fmt::Display for ApiErrors<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.first() {
            Some(e) => {
                match &e.message {
                    Some(message) => write!(f, "{}: {message}", e.code),
                    None => write!(f, "{}", e.code),
                }
            }
            None => write!(f, "request failed without an error code"),
        }
    }
}
