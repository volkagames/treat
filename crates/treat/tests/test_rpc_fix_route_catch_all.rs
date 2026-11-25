//! `treat::rpc::route` with a catch-all `{*name}` segment: it takes the
//! non-empty rest of the path, one segment or several, and ranks after a fixed
//! segment — as axum's router (`matchit`) does, which the last test checks
//! against a real router. axum refuses a `{name}` and a `{*name}` at one place,
//! so the table holds no such pair.

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::get;
use tower::ServiceExt;
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

const fn op(id: &'static str, path: &'static str) -> Operation {
    Operation {
        id,
        method: "GET",
        path,
    }
}

/// Sorted by path and then method, as a generator emits it.
const ALL: &[Operation] = &[
    op("assets", "/assets/{*path}"),
    op("blank", "/files/new"),
    op("new_one", "/files/new/{id}"),
    op("rest", "/files/{*rest}"),
    op("repo_tree", "/{org}/{repo}/{*tree}"),
];

fn id(path: &str) -> Option<&'static str> {
    route::by_route(ALL, "GET", path).map(|row| row.id)
}

#[test]
fn the_table_is_well_formed() {
    assert_eq!(route::check_table(ALL), Ok(()));
}

#[test]
fn a_catch_all_takes_one_segment_or_several() {
    assert!(route::fits("/files/{*rest}", "/files/a"));
    assert!(route::fits("/files/{*rest}", "/files/a/b"));
    assert!(
        route::fits("/files/{*rest}", "/files/a/b/"),
        "a trailing `/` is part of the rest"
    );
    assert!(
        route::fits("/files/{*rest}", "/files/a//b"),
        "so is an empty segment inside it"
    );
    assert!(route::fits("/{*rest}", "/a"));
    assert!(route::fits("/{org}/{*rest}", "/o/a/b"));
}

#[test]
fn a_catch_all_takes_no_empty_rest() {
    assert!(!route::fits("/files/{*rest}", "/files/"));
    assert!(!route::fits("/files/{*rest}", "/files"));
    assert!(!route::fits("/{*rest}", "/"));
    assert!(!route::fits("/{*rest}", ""));
}

#[test]
fn the_segments_before_a_catch_all_fit_as_usual() {
    assert!(!route::fits("/files/{*rest}", "/other/a"));
    assert!(
        !route::fits("/{org}/{*rest}", "//a"),
        "`{{org}}` takes no empty segment"
    );
    assert!(!route::fits("/{org}/{*rest}", "/o"));
}

#[test]
fn a_named_parameter_is_still_one_segment() {
    assert!(route::fits("/files/{id}", "/files/a"));
    assert!(!route::fits("/files/{id}", "/files/a/b"));
}

#[test]
fn a_fixed_segment_wins_over_a_catch_all() {
    assert_eq!(id("/files/new"), Some("blank"));
    assert_eq!(id("/files/new/1"), Some("new_one"));
    assert_eq!(id("/files/new/1/2"), Some("rest"), "`{{id}}` takes one segment");
    assert_eq!(id("/files/new/"), Some("rest"), "`{{id}}` takes no empty one");
    assert_eq!(id("/files/newer"), Some("rest"));
    assert_eq!(id("/files/a"), Some("rest"));
    assert_eq!(id("/files/a/b"), Some("rest"));
    assert_eq!(id("/files/"), None);
    assert_eq!(id("/assets/css/site.css"), Some("assets"));
    assert_eq!(id("/assets/"), None);
    assert_eq!(id("/o/r/src/main.rs"), Some("repo_tree"));
    assert_eq!(id("/o/r"), None);
}

#[test]
fn a_catch_all_template_is_found_as_spelled() {
    assert_eq!(id("/files/{*rest}"), Some("rest"));
}

/// Every path above, routed by axum: the row `by_route` picks is the route
/// the router runs, and `None` is the router's `404`.
#[tokio::test]
async fn the_router_picks_the_same_route() {
    let mut router = Router::new();
    for row in ALL {
        let id = row.id;
        router = router.route(row.path, get(move || std::future::ready(id)));
    }
    let paths = [
        "/files/new",
        "/files/new/1",
        "/files/new/1/2",
        "/files/new/",
        "/files/newer",
        "/files/a",
        "/files/a/b",
        "/files/a/b/",
        "/files/a//b",
        "/files/",
        "/files",
        "/assets/css/site.css",
        "/assets/",
        "/assets",
        "/o/r/src/main.rs",
        "/o/r",
        "/o/r/",
    ];
    for path in paths {
        let request = Request::get(path).body(Body::empty()).expect("a request");
        let response = router.clone().oneshot(request).await.expect("the router answers");
        let routed = match response.status() {
            StatusCode::OK => {
                let bytes = axum::body::to_bytes(response.into_body(), 1024).await.expect("a body");
                Some(String::from_utf8(bytes.to_vec()).expect("text"))
            }
            StatusCode::NOT_FOUND => None,
            other => panic!("{path}: {other}"),
        };
        assert_eq!(id(path).map(str::to_string), routed, "{path}");
    }
}
