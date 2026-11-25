/// Emit a `tracing` event at a level known only at runtime.
///
/// `tracing::event!` needs its level as a compile-time constant, so a level held
/// in a variable cannot be passed straight through. This expands to one arm per
/// level and dispatches on the value, keeping each `event!` call constant.
///
/// Takes the same optional `target:` / `parent:` prefixes as `tracing::event!`.
/// The expansion reaches `tracing` through this crate, so the calling crate
/// needs no `tracing` dependency of its own.
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
            $crate::__private::tracing::Level::ERROR => {
                $crate::__private::tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    $crate::__private::tracing::Level::ERROR,
                    $($tt)*
                );
            }
            $crate::__private::tracing::Level::WARN => {
                $crate::__private::tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    $crate::__private::tracing::Level::WARN,
                    $($tt)*
                );
            }
            $crate::__private::tracing::Level::INFO => {
                $crate::__private::tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    $crate::__private::tracing::Level::INFO,
                    $($tt)*
                );
            }
            $crate::__private::tracing::Level::DEBUG => {
                $crate::__private::tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    $crate::__private::tracing::Level::DEBUG,
                    $($tt)*
                );
            }
            $crate::__private::tracing::Level::TRACE => {
                $crate::__private::tracing::event!(
                    $(target: $target,)?
                    $(parent: $parent,)?
                    $crate::__private::tracing::Level::TRACE,
                    $($tt)*
                );
            }
        }
    };
}
