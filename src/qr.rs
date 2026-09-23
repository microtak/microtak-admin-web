//! Renders an enrollment token as an inline SVG QR code -- no client-side
//! JS QR library, no PNG round-trip, generated server-side since the token
//! value already exists there.
//!
//! **Payload scheme (MicroTAK's own, not a claimed-compatible ATAK/Marti
//! standard -- no authoritative source for a real one was found)**: a JSON
//! object `{"microtakEnroll": {"token": "<token>", "enrollmentUrl":
//! "<url or omitted>"}}`. `enrollmentUrl` is included only if
//! `MICROTAK_ADMIN_WEB_ENROLLMENT_URL` is configured; a provisioning tool
//! without it needs to already know which server to enroll against. This
//! is deliberately simple/inspectable (plain JSON, not a bespoke binary
//! encoding) so any future `microtak-node`/`microtak-admin-cli` consumer
//! can parse it trivially, and a human can read it off a phone's raw QR
//! scan result to sanity-check what they're about to submit.

use maud::{PreEscaped, Render};
use qrcode::render::svg;
use qrcode::QrCode;

pub struct EnrollmentQr {
    pub token: String,
    pub enrollment_url: Option<String>,
}

impl EnrollmentQr {
    fn payload(&self) -> String {
        let mut inner = serde_json::json!({ "token": self.token });
        if let Some(url) = &self.enrollment_url {
            inner["enrollmentUrl"] = serde_json::Value::String(url.clone());
        }
        serde_json::json!({ "microtakEnroll": inner }).to_string()
    }
}

impl Render for EnrollmentQr {
    fn render(&self) -> maud::Markup {
        let code = QrCode::new(self.payload().as_bytes()).expect("token payload is well within QR capacity");
        let svg = code
            .render()
            .min_dimensions(240, 240)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build();
        PreEscaped(svg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_includes_token_and_optional_enrollment_url() {
        let qr = EnrollmentQr {
            token: "abc123".to_string(),
            enrollment_url: Some("http://server:8446".to_string()),
        };
        let payload = qr.payload();
        let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
        assert_eq!(parsed["microtakEnroll"]["token"], "abc123");
        assert_eq!(parsed["microtakEnroll"]["enrollmentUrl"], "http://server:8446");
    }

    #[test]
    fn payload_omits_enrollment_url_when_not_configured() {
        let qr = EnrollmentQr {
            token: "abc123".to_string(),
            enrollment_url: None,
        };
        let parsed: serde_json::Value = serde_json::from_str(&qr.payload()).unwrap();
        assert!(parsed["microtakEnroll"].get("enrollmentUrl").is_none());
    }

    #[test]
    fn renders_non_empty_svg() {
        let qr = EnrollmentQr {
            token: "a".repeat(64),
            enrollment_url: None,
        };
        let markup = qr.render().into_string();
        assert!(markup.contains("<svg"));
    }
}
