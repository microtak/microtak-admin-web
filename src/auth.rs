//! HTTP Basic Auth gate on every admin page.
//!
//! **Deliberate v1 simplification, documented rather than silently
//! shipped**: this is a single shared secret (`MICROTAK_ADMIN_WEB_PASSWORD`),
//! not a real user-account system -- there's no per-operator identity, no
//! audit trail of *who* revoked a token or reassigned a mission role
//! through this UI (only that the shared admin mTLS identity did, same as
//! it would via raw `curl`), and no password rotation/expiry. This tool
//! holds a powerful mTLS admin credential and can take destructive actions
//! (revoke tokens, strip a mission's owner role), so *some* gate is
//! required before anyone can reach it at all -- this is that gate, not a
//! finished access-control system. A real multi-operator deployment should
//! run this behind its own network-level access control (VPN, reverse
//! proxy with real auth) in addition to, not instead of, this check.

use axum::extract::Request;
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

pub async fn require_password(
    axum::extract::State(expected_password): axum::extract::State<std::sync::Arc<String>>,
    request: Request,
    next: Next,
) -> Response {
    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_basic_auth_password);

    match provided {
        Some(password) if constant_time_eq(password.as_bytes(), expected_password.as_bytes()) => {
            next.run(request).await
        }
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Basic realm=\"microtak-admin-web\"")],
            "authentication required",
        )
            .into_response(),
    }
}

fn parse_basic_auth_password(header_value: &str) -> Option<String> {
    let encoded = header_value.strip_prefix("Basic ")?;
    let decoded = base64_decode(encoded)?;
    let text = String::from_utf8(decoded).ok()?;
    // Username is ignored entirely -- only the password is the actual
    // shared secret; any username value is accepted.
    let (_username, password) = text.split_once(':')?;
    Some(password.to_string())
}

/// Byte-for-byte equal-length comparison that doesn't short-circuit on the
/// first mismatching byte -- avoids leaking how many leading characters of
/// a guess were correct via response-timing differences. Not a substitute
/// for the password being a real secret in the first place.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A minimal base64 decoder covering exactly what HTTP Basic Auth needs
/// (standard alphabet, `=` padding) -- avoids pulling in a whole base64
/// crate for one decode call.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut lookup = [255u8; 256];
    for (i, &c) in ALPHABET.iter().enumerate() {
        lookup[c as usize] = i as u8;
    }

    let input = input.trim_end_matches('=');
    let mut bits: u32 = 0;
    let mut bit_count = 0;
    let mut out = Vec::with_capacity(input.len() * 3 / 4 + 1);

    for byte in input.bytes() {
        let value = lookup[byte as usize];
        if value == 255 {
            return None;
        }
        bits = (bits << 6) | value as u32;
        bit_count += 6;
        if bit_count >= 8 {
            bit_count -= 8;
            out.push((bits >> bit_count) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_standard_basic_auth_header() {
        // "admin:hunter2" base64-encoded.
        let header = "Basic YWRtaW46aHVudGVyMg==";
        let password = parse_basic_auth_password(header).unwrap();
        assert_eq!(password, "hunter2");
    }

    #[test]
    fn rejects_non_basic_scheme() {
        assert!(parse_basic_auth_password("Bearer sometoken").is_none());
    }

    #[test]
    fn constant_time_eq_matches_equal_slices() {
        assert!(constant_time_eq(b"secret", b"secret"));
    }

    #[test]
    fn constant_time_eq_rejects_different_slices() {
        assert!(!constant_time_eq(b"secret", b"wrong!"));
        assert!(!constant_time_eq(b"short", b"muchlonger"));
    }
}
