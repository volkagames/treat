use crate::{ApiError, ApiErrorCode, ApiResponse, ErrorMessage, NoMeta, ResponseData};

impl<C: ApiErrorCode> ApiError<C> {
    /// Build the client-facing response. By default only the top-level error is
    /// serialized into `errors`; the full cause chain is included only in
    /// verbose mode (via [`ApiError::with_verbose`] or the `verbose-error`
    /// feature). This is the single entry point shared by every framework
    /// adapter, so the wire format is identical across actix and axum.
    pub fn into_api_response<T: ResponseData>(&self) -> ApiResponse<T, NoMeta> {
        let errors = if self.is_verbose() {
            self.collect_messages()
        } else {
            vec![self.to_error_message()]
        };
        ApiResponse {
            data: None,
            meta: None,
            errors,
        }
    }

    /// Collect the top-level error together with every `ApiError` found in its
    /// cause chain. Always returns the full chain regardless of verbose mode.
    ///
    /// Each link that is itself an `ApiError` contributes its own `message`
    /// verbatim. Folding the source chain into every entry (as
    /// [`format_message_verbose`](ApiError::format_message_verbose) does for logs)
    /// would repeat each cause once per ancestor and leak the `Display` prefix
    /// into the payload — here the chain *is* the list.
    ///
    /// A cause that is **not** an `ApiError` (an `erris::Report` from
    /// `wrap_api_error`, `?` on a foreign error, ...) carries no `code`, so it
    /// cannot become a list entry of its own. Dropping it would make verbose mode
    /// a silent no-op for the most common way a cause is attached, so such text
    /// is appended to the message of the nearest `ApiError` above it, separated
    /// by `", "`.
    pub fn collect_messages(&self) -> Vec<ErrorMessage> {
        let mut output = vec![self.to_error_message_with(false)];
        if let Some(report) = self.source() {
            collect_api_errors::<C>(report, false, &mut output, &mut Vec::new());
        }
        output
    }

    #[cfg(feature = "spantrace")]
    pub fn spantrace(&self) -> &erris::SpanTrace {
        &self.spantrace
    }
}

pub fn report_collect_messages<C: ApiErrorCode + 'static>(report: &erris::Report, verbose: bool) -> Vec<ErrorMessage> {
    let mut output = vec![];
    let mut seen = vec![];
    collect_api_errors::<C>(report, verbose, &mut output, &mut seen);
    output
}

// `report.chain()` follows only the `source()` (cause) branch once it crosses a
// foreign-error boundary — our `ApiError` is foreign to erris — so the `message`
// branch of a `Wrapper` added via `with_err` would be dropped. Recurse into each
// `ApiError`'s own source report with a fresh `chain()` (which expands both
// wrapper branches from an owned root), deduplicating by identity.
//
// `downcast_ref` is a `TypeId` check, so it must name the *concrete* `ApiError<C>`.
// A chain built from a service's own errors is homogeneous in `C`, but library
// helpers commonly nest an `ApiError<&'static str>` (the default) inside a typed
// error. Try the caller's `C` at every node, then fall back to the default code
// type, so neither is silently dropped.
fn collect_api_errors<C: ApiErrorCode + 'static>(
    report: &erris::Report,
    verbose: bool,
    output: &mut Vec<ErrorMessage>,
    seen: &mut Vec<*const ()>,
) {
    for cause in report.chain() {
        let err = cause.as_error();
        let matched = err
            .downcast_ref::<ApiError<C>>()
            .map(|e| {
                (
                    e as *const ApiError<C> as *const (),
                    e.to_error_message_with(verbose),
                    e.source(),
                )
            })
            .or_else(|| {
                err.downcast_ref::<ApiError<&'static str>>().map(|e| {
                    (
                        e as *const ApiError<&'static str> as *const (),
                        e.to_error_message_with(verbose),
                        e.source(),
                    )
                })
            });
        let Some((id, message, source)) = matched else {
            // A foreign cause: no `code`, so it gets no entry of its own.
            // Append its text to the nearest `ApiError` above it rather than
            // dropping it — that entry is the only place it can surface.
            append_foreign_cause(err, output);
            continue;
        };
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);
        output.push(message);
        if let Some(source) = source {
            collect_api_errors::<C>(source, verbose, output, seen);
        }
    }
}

/// Joins a foreign cause onto the message of the `ApiError` entry above it.
/// Matches the separator [`ApiError::format_message_verbose`] uses, so the two
/// verbose renderings read the same.
const FOREIGN_CAUSE_SEPARATOR: &str = ", ";

/// Append a non-`ApiError` cause's text to the last collected entry.
///
/// Such a cause has no `code`, so it cannot be an `errors[]` entry; folding it
/// into the nearest `ApiError` above is the only way it reaches the client.
///
/// Only **leaf** nodes are folded. `chain()` also yields erris' own wrapper
/// nodes, which are foreign to us but merely re-`Display` the error they carry —
/// for a wrapped `ApiError` that renders the internal `"leto error: ..."` prefix,
/// and that child already contributes its own entry. A wrapper is recognised by
/// having a `source()`; a genuine foreign leaf has none.
///
/// Also a no-op when there is no entry to attach to (the cause sits above every
/// `ApiError` in the chain — the caller's own top-level entry already covers it)
/// or when the text is empty, which is how erris renders the transparent
/// wrappers that [`ApiError::track`] inserts.
fn append_foreign_cause(err: &(dyn std::error::Error + 'static), output: &mut [ErrorMessage]) {
    if err.source().is_some() {
        return;
    }
    let Some(last) = output.last_mut() else {
        return;
    };
    let text = err.to_string();
    if text.is_empty() {
        return;
    }
    match &mut last.message {
        // Guard against re-appending text the entry already carries: the same
        // cause can be reached twice when a wrapper expands both its branches.
        Some(message) => {
            if !message.contains(&text) {
                message.push_str(FOREIGN_CAUSE_SEPARATOR);
                message.push_str(&text);
            }
        }
        None => last.message = Some(text),
    }
}

impl From<ErrorMessage> for erris::Report {
    #[track_caller]
    fn from(err: ErrorMessage) -> Self {
        erris::report!("{}", err)
    }
}
