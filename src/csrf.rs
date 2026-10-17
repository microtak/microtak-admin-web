//! Cross-site request forgery guard for every state-changing request.
//!
//! The UI authenticates with HTTP Basic Auth (see [`crate::auth`]), and
//! browsers attach those credentials to *any* request to this origin --
//! also a form post that a third-party page makes the admin's browser
//! submit. Such a post could revoke tokens, delete groups or change
//! mission roles. So every unsafe request (anything but GET/HEAD/OPTIONS)
//! must show it comes from this UI itself:
//!
//! 1. `Sec-Fetch-Site` (sent by all current browsers): must be
//!    `same-origin`.
//! 2. Otherwise `Origin`, or failing that `Referer`: its host and port must
//!    equal the request's `Host`.
//! 3. No provenance at all: refused. This UI is for browsers; scripts use
//!    microtak-admin-cli or the server's admin API directly.

use axum::extract::Request;
use axum::http::{header, HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn require_same_origin(request: Request, next: Next) -> Response {
    if is_safe(request.method()) || comes_from_this_ui(request.headers()) {
        next.run(request).await
    } else {
        tracing::warn!(method = %request.method(), uri = %request.uri(), "refused a cross-site request");
        (StatusCode::FORBIDDEN, "cross-site request refused").into_response()
    }
}

fn is_safe(method: &Method) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
}

fn comes_from_this_ui(headers: &HeaderMap) -> bool {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(site) = header("sec-fetch-site") {
        return site == "same-origin";
    }
    let Some(host) = header(header::HOST.as_str()) else {
        return false;
    };
    match header(header::ORIGIN.as_str()).or_else(|| header(header::REFERER.as_str())) {
        Some(source) => authority_of(source).is_some_and(|authority| authority.eq_ignore_ascii_case(host)),
        None => false,
    }
}

/// `host[:port]` of an absolute URL (`Origin` or `Referer`).
fn authority_of(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    (!authority.is_empty()).then_some(authority)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(*name, value.parse().unwrap());
        }
        map
    }

    #[test]
    fn origin_must_match_host_including_port() {
        assert!(comes_from_this_ui(&headers(&[("host", "a:8090"), ("origin", "http://a:8090")])));
        assert!(!comes_from_this_ui(&headers(&[("host", "a:8090"), ("origin", "http://a:9999")])));
        assert!(!comes_from_this_ui(&headers(&[("host", "a:8090"), ("origin", "null")])));
    }

    #[test]
    fn sec_fetch_site_wins_over_a_matching_origin() {
        assert!(!comes_from_this_ui(&headers(&[
            ("host", "a"),
            ("origin", "http://a"),
            ("sec-fetch-site", "cross-site"),
        ])));
    }

    #[test]
    fn referer_is_the_fallback() {
        assert!(comes_from_this_ui(&headers(&[("host", "a"), ("referer", "https://a/tokens?x=1")])));
        assert!(!comes_from_this_ui(&headers(&[("host", "a"), ("referer", "https://a.evil/tokens")])));
    }

    #[test]
    fn no_provenance_is_refused() {
        assert!(!comes_from_this_ui(&headers(&[("host", "a")])));
    }
}
