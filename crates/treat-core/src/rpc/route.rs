//! Find the operation a router mounts at a request's method and path.
//!
//! The table is any slice of rows implementing [`RouteRow`], **sorted by path
//! and then method**, with two rows sharing a path only under different
//! methods — the order a generator emits it in. That order is what lets a path
//! some row spells exactly be found by binary search; only a path no row spells
//! scans the rows with a parameter.
//!
//! The lookups trust that order rather than checking it on every request:
//! [`check_table`] checks it once, from a test of the code that owns the table.
//!
//! A row's path is the path the router mounts, `{name}` segments and all; its
//! method is upper-case as HTTP spells it, or [`ANY`] for a route that answers
//! every method.

/// The method of a row that answers every method.
pub const ANY: &str = "ANY";

/// One row of an operation table, as the router mounts it.
pub trait RouteRow {
    /// `GET`, `POST`, ... upper-case as HTTP spells it, or [`ANY`].
    fn method(&self) -> &str;
    /// The path the router mounts, `{name}` segments and all.
    fn path(&self) -> &str;
}

/// The row the router mounts at `method path`; `None` for a route it does not,
/// so a caller that refuses on `None` fails closed.
///
/// `method` is matched as spelled: `HEAD`, which a router may answer for a
/// `GET` route, is no row, and is refused rather than read as the `GET`. An
/// [`ANY`] row answers every method, as its route does.
///
/// `path` is the request's (`/api/user/u1`) or the route's
/// (`/api/user/{user_id}`, a router's matched path): a `{name}` segment of a row
/// takes any one non-empty segment. The route is chosen as the router chooses
/// it, a fixed segment before a parameter, and only then the method: a method
/// the chosen route lacks is `None`, as the router answers it `405`, even where
/// another route would fit.
///
/// A path some row spells exactly — every segment fixed, or a route's own
/// template — is a binary search; only a path no row spells scans the rows with
/// a parameter. `rows` must be in [`check_table`]'s order; an unsorted table is
/// not detected here and gives wrong answers.
pub fn by_route<'a, Row: RouteRow>(rows: &'a [Row], method: &str, path: &str) -> Option<&'a Row> {
    let mut route = rows_at(rows, path).first().map(RouteRow::path);
    if route.is_none() {
        for row in rows.iter().filter(|row| row.path().contains('{')) {
            let closer = match route {
                Some(chosen) => more_fixed(row.path(), chosen),
                None => true,
            };
            if closer && fits(row.path(), path) {
                route = Some(row.path());
            }
        }
    }
    rows_at(rows, route?)
        .iter()
        .find(|row| row.method() == method || row.method() == ANY)
}

/// Whether `rows` is a table [`by_route`] can search: sorted by path and then
/// method, no `(path, method)` twice, and an [`ANY`] row alone on its path (a
/// router mounting `ANY` and `GET` at one path answers `GET` with one of them,
/// and which one is the router's business, not the table's).
///
/// `Err` names the first offending rows. Meant for a test of the code that owns
/// the table — a generator's output or a hand-written one — rather than for a
/// request path: the table is fixed at build time, so one check covers every
/// lookup.
pub fn check_table<Row: RouteRow>(rows: &[Row]) -> Result<(), String> {
    for pair in rows.windows(2) {
        let [prev, next] = pair else {
            continue;
        };
        let (prev_path, prev_method) = (prev.path(), prev.method());
        let (next_path, next_method) = (next.path(), next.method());
        if (prev_path, prev_method) > (next_path, next_method) {
            return Err(format!(
                "`{next_method} {next_path}` comes after `{prev_method} {prev_path}`; sort the table by path and then \
                 method"
            ));
        }
        if prev_path != next_path {
            continue;
        }
        if prev_method == next_method {
            return Err(format!("`{next_method} {next_path}` is in the table twice"));
        }
        if prev_method == ANY || next_method == ANY {
            return Err(format!(
                "`{prev_method} {prev_path}` and `{next_method} {next_path}` share a path, but an `{ANY}` row answers \
                 every method and must be alone on its path"
            ));
        }
    }
    Ok(())
}

/// The rows mounted at exactly `path`, one per method; empty for a path no row
/// spells. `rows` must be sorted by path.
pub fn rows_at<'a, Row: RouteRow>(rows: &'a [Row], path: &str) -> &'a [Row] {
    let start = rows.partition_point(|row| row.path() < path);
    let rows = rows.get(start..).unwrap_or_default();
    let len = rows.partition_point(|row| row.path() == path);
    rows.get(..len).unwrap_or_default()
}

/// Whether `path` is a path the route `route` matches: as many segments, each
/// spelled the same or taken by a `{name}` segment, which takes exactly one
/// non-empty segment.
pub fn fits(route: &str, path: &str) -> bool {
    route.split('/').count() == path.split('/').count()
        && route
            .split('/')
            .zip(path.split('/'))
            .all(|(want, got)| want == got || (want.starts_with('{') && !got.is_empty()))
}

/// Whether the route `a` has a fixed segment where `b` has its first parameter
/// the two do not share: what the router tries first.
fn more_fixed(a: &str, b: &str) -> bool {
    for (x, y) in a.split('/').zip(b.split('/')) {
        match (x.starts_with('{'), y.starts_with('{')) {
            (false, true) => return true,
            (true, false) => return false,
            _ => {}
        }
    }
    false
}
