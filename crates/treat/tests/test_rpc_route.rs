//! `treat::rpc::route`: the row of an operation table the router would pick
//! for a request's method and path.

use treat::rpc::route::{self, RouteRow};

struct Operation {
    id: &'static str,
    method: &'static str,
    path: &'static str,
}

impl RouteRow for Operation {
    fn method(&self) -> &str {
        self.method
    }

    fn path(&self) -> &str {
        self.path
    }
}

const fn op(id: &'static str, method: &'static str, path: &'static str) -> Operation {
    Operation { id, method, path }
}

/// Sorted by path and then method, as a generator emits it.
const ALL: &[Operation] = &[
    op("root", "GET", "/"),
    op("a_fixed_param", "POST", "/a/b/{y}"),
    op("a_param_fixed", "GET", "/a/{x}/c"),
    op("info", "ANY", "/api/info"),
    op("blank", "GET", "/api/user/new"),
    op("drop", "DELETE", "/api/user/{user_id}"),
    op("user", "GET", "/api/user/{user_id}"),
    op("by_email", "GET", "/api/user_by_email"),
    op("whoami", "GET", "/api/whoami"),
    op("rename", "PUT", "/items/{id}"),
    op("thing", "ANY", "/things/{id}"),
];

fn id(method: &str, path: &str) -> Option<&'static str> {
    route::by_route(ALL, method, path).map(|row| row.id)
}

#[test]
fn fixed_path_is_found() {
    assert_eq!(id("GET", "/api/whoami"), Some("whoami"));
    assert_eq!(id("GET", "/api/user_by_email"), Some("by_email"));
}

#[test]
fn parameter_takes_one_segment() {
    assert_eq!(id("GET", "/api/user/u1"), Some("user"));
    assert_eq!(id("DELETE", "/api/user/u1"), Some("drop"));
    assert_eq!(id("PUT", "/items/42"), Some("rename"));
}

#[test]
fn route_template_is_found_as_spelled() {
    assert_eq!(id("GET", "/api/user/{user_id}"), Some("user"));
    assert_eq!(id("DELETE", "/api/user/{user_id}"), Some("drop"));
}

#[test]
fn fixed_segment_wins_over_a_parameter() {
    assert_eq!(id("GET", "/api/user/new"), Some("blank"));
    // `/a/b/{y}` is fixed where `/a/{x}/c` has its first parameter.
    assert_eq!(id("POST", "/a/b/c"), Some("a_fixed_param"));
}

#[test]
fn method_the_chosen_route_lacks_is_none() {
    // `/api/user/{user_id}` has a DELETE, but the router picks `/api/user/new`
    // and answers 405.
    assert_eq!(id("DELETE", "/api/user/new"), None);
    // `/a/{x}/c` has a GET, but `/a/b/{y}` is the route the router picks.
    assert_eq!(id("GET", "/a/b/c"), None);
    assert_eq!(id("POST", "/api/whoami"), None);
}

#[test]
fn any_answers_every_method() {
    for method in ["GET", "POST", "PUT", "DELETE", "HEAD", "OPTIONS"] {
        assert_eq!(id(method, "/api/info"), Some("info"), "{method}");
    }
}

#[test]
fn head_is_not_get() {
    assert_eq!(id("HEAD", "/api/whoami"), None);
    assert_eq!(id("HEAD", "/api/user/u1"), None);
}

#[test]
fn method_is_matched_as_spelled() {
    assert_eq!(id("get", "/api/whoami"), None);
}

#[test]
fn empty_segment_is_no_parameter() {
    assert_eq!(id("GET", "/api/user/"), None);
    assert_eq!(id("GET", "/api/user//"), None);
}

#[test]
fn segment_count_must_match() {
    assert_eq!(id("GET", "/api/user/u1/extra"), None);
    assert_eq!(id("GET", "/api/user"), None);
}

#[test]
fn unknown_path_is_none() {
    assert_eq!(id("GET", "/nope"), None);
    assert_eq!(id("GET", ""), None);
    assert_eq!(
        route::by_route::<Operation>(&[], "GET", "/api/info").map(|row| row.id),
        None
    );
}

#[test]
fn rows_at_lists_every_method_of_a_route() {
    let rows: Vec<_> = route::rows_at(ALL, "/api/user/{user_id}")
        .iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(rows, ["drop", "user"]);
    assert!(route::rows_at(ALL, "/api/user/u1").is_empty());
}

#[test]
fn fits_matches_a_path_against_a_route() {
    assert!(route::fits("/api/user/{user_id}", "/api/user/u1"));
    assert!(route::fits("/api/user/{user_id}", "/api/user/{user_id}"));
    assert!(!route::fits("/api/user/{user_id}", "/api/user/"));
    assert!(!route::fits("/api/user/{user_id}", "/api/users/u1"));
    assert!(route::fits("/api/whoami", "/api/whoami"));
}

#[test]
fn table_is_well_formed() {
    assert_eq!(route::check_table(ALL), Ok(()));
}

#[test]
fn any_on_a_parameterised_route_answers_every_method() {
    for method in ["GET", "POST", "DELETE", "HEAD"] {
        assert_eq!(id(method, "/things/t1"), Some("thing"), "{method}");
    }
    assert_eq!(id("PATCH", "/things/{id}"), Some("thing"));
    assert_eq!(id("GET", "/things/"), None);
}

#[test]
fn root_path_is_a_route_of_its_own() {
    assert_eq!(id("GET", "/"), Some("root"));
    assert_eq!(id("POST", "/"), None);
    assert_eq!(id("GET", "//"), None);
}

/// A trailing slash is one more, empty, segment: the router mounts
/// `/api/whoami` and not `/api/whoami/`, and no parameter takes the empty one.
#[test]
fn trailing_slash_is_another_path() {
    assert_eq!(id("GET", "/api/whoami/"), None);
    assert_eq!(id("GET", "/items/42/"), None);
}

/// A path spelled as a template the table does not have is a request path like
/// any other: `{id}` is a segment the row's `{user_id}` takes, as a router would
/// take it.
#[test]
fn other_template_is_matched_as_a_request_path() {
    assert_eq!(id("GET", "/api/user/{id}"), Some("user"));
    assert!(route::rows_at(ALL, "/api/user/{id}").is_empty());
}

#[test]
fn check_table_accepts_the_empty_table() {
    assert_eq!(route::check_table::<Operation>(&[]), Ok(()));
}

#[test]
fn check_table_refuses_an_unsorted_path() {
    let rows = [op("b", "GET", "/b"), op("a", "GET", "/a")];
    let err = route::check_table(&rows).expect_err("unsorted");
    assert!(err.contains("`GET /a` comes after `GET /b`"), "{err}");
}

#[test]
fn check_table_refuses_unsorted_methods_of_one_path() {
    let rows = [op("put", "PUT", "/a"), op("get", "GET", "/a")];
    let err = route::check_table(&rows).expect_err("unsorted");
    assert!(err.contains("`GET /a` comes after `PUT /a`"), "{err}");
}

#[test]
fn check_table_refuses_a_duplicate_route() {
    let rows = [op("first", "GET", "/a"), op("second", "GET", "/a")];
    let err = route::check_table(&rows).expect_err("duplicate");
    assert!(err.contains("`GET /a` is in the table twice"), "{err}");
}

#[test]
fn check_table_refuses_any_beside_another_method() {
    let rows = [op("any", "ANY", "/a/{id}"), op("get", "GET", "/a/{id}")];
    let err = route::check_table(&rows).expect_err("ANY shares its path");
    assert!(err.contains("must be alone on its path"), "{err}");
    // `ANY` after the other method: WebDAV's `ACL` sorts before it.
    let rows = [op("acl", "ACL", "/a"), op("any", "ANY", "/a")];
    let err = route::check_table(&rows).expect_err("ANY shares its path");
    assert!(err.contains("must be alone on its path"), "{err}");
}

#[test]
fn check_table_accepts_any_alone_and_methods_sharing_a_path() {
    let rows = [
        op("any", "ANY", "/a"),
        op("drop", "DELETE", "/b/{id}"),
        op("get", "GET", "/b/{id}"),
    ];
    assert_eq!(route::check_table(&rows), Ok(()));
}
