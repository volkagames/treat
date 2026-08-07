/// Emit a `tracing` event at a level known only at runtime.
///
/// `tracing::event!` needs its level as a compile-time constant, so a level held
/// in a variable cannot be passed straight through. This expands to one arm per
/// level and dispatches on the value, keeping each `event!` call constant.
///
/// Takes the same optional `target:` / `parent:` prefixes as `tracing::event!`.
///
/// ```
/// # use treat_axum_logging::event_dynamic_lvl;
/// let level = if cfg!(debug_assertions) {
///     tracing::Level::DEBUG
/// } else {
///     tracing::Level::INFO
/// };
/// event_dynamic_lvl!(level, message = "served", status = 200);
/// ```
#[macro_export]
macro_rules! event_dynamic_lvl {
    ( $(target: $target:expr,)? $(parent: $parent:expr,)? $lvl:expr, $($tt:tt)* ) => {
        match $lvl {
            tracing::Level::ERROR => {
                tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    tracing::Level::ERROR,
                    $($tt)*
                );
            }
            tracing::Level::WARN => {
                tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    tracing::Level::WARN,
                    $($tt)*
                );
            }
            tracing::Level::INFO => {
                tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    tracing::Level::INFO,
                    $($tt)*
                );
            }
            tracing::Level::DEBUG => {
                tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    tracing::Level::DEBUG,
                    $($tt)*
                );
            }
            tracing::Level::TRACE => {
                tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    tracing::Level::TRACE,
                    $($tt)*
                );
            }
        }
    };
}
