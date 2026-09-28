//! Enrollment QR codes in the format TAK clients actually scan, rendered
//! server-side as inline SVG (no client-side JS, no PNG round-trip).
//!
//! Payload: the standard ATAK enrollment deep link,
//! `tak://com.atakmap.app/enroll?host=<host>&username=<name>&token=<token>`
//! -- scanning it makes ATAK / OmniTAK enroll over HTTPS against `host`,
//! presenting `token` as the password for `username` (microtak-server
//! accepts an invite token bound to that device name that way). OmniTAK
//! also reads `enrollmentport=`, `port=` (streaming) and `apiport=`; they're
//! only added when they differ from the TAK defaults (8446/8089/8443). Same
//! format as `microtak-admin-cli token mint --qr`.

use maud::{PreEscaped, Render};
use qrcode::render::svg;
use qrcode::QrCode;

pub const DEFAULT_ENROLLMENT_PORT: u16 = 8446;
pub const DEFAULT_STREAMING_PORT: u16 = 8089;
pub const DEFAULT_API_PORT: u16 = 8443;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrollmentLink {
    pub host: String,
    pub enrollment_port: u16,
    pub streaming_port: u16,
    pub api_port: u16,
    pub username: String,
    pub token: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("the enrollment URL must be https:// (got '{0}')")]
    NotHttps(String),
    #[error("the enrollment URL has no host or an invalid port: '{0}'")]
    Malformed(String),
    #[error("IPv6 literals aren't supported in enrollment QR codes -- use a DNS name or IPv4 address")]
    Ipv6Literal,
}

impl EnrollmentLink {
    /// Build from the server's enrollment URL (`https://host[:port]`, port
    /// defaulting to 443 -- e.g. behind a reverse proxy).
    pub fn new(
        enrollment_url: &str,
        streaming_port: u16,
        api_port: u16,
        username: &str,
        token: &str,
    ) -> Result<Self, LinkError> {
        let rest = enrollment_url
            .get(..8)
            .filter(|scheme| scheme.eq_ignore_ascii_case("https://"))
            .map(|_| &enrollment_url[8..])
            .ok_or_else(|| LinkError::NotHttps(enrollment_url.to_string()))?;
        let authority = rest.split('/').next().unwrap_or_default();
        if authority.starts_with('[') {
            return Err(LinkError::Ipv6Literal);
        }
        let (host, enrollment_port) = match authority.rsplit_once(':') {
            Some((host, port)) => (
                host,
                port.parse()
                    .map_err(|_| LinkError::Malformed(enrollment_url.to_string()))?,
            ),
            None => (authority, 443),
        };
        if host.is_empty() {
            return Err(LinkError::Malformed(enrollment_url.to_string()));
        }
        Ok(Self {
            host: host.to_string(),
            enrollment_port,
            streaming_port,
            api_port,
            username: username.to_string(),
            token: token.to_string(),
        })
    }

    pub fn to_uri(&self) -> String {
        let mut uri = format!(
            "tak://com.atakmap.app/enroll?host={}&username={}&token={}",
            encode(&self.host),
            encode(&self.username),
            encode(&self.token)
        );
        if self.enrollment_port != DEFAULT_ENROLLMENT_PORT {
            uri.push_str(&format!("&enrollmentport={}", self.enrollment_port));
        }
        if self.streaming_port != DEFAULT_STREAMING_PORT {
            uri.push_str(&format!("&port={}", self.streaming_port));
        }
        if self.api_port != DEFAULT_API_PORT {
            uri.push_str(&format!("&apiport={}", self.api_port));
        }
        uri
    }
}

/// Percent-encode everything outside RFC 3986's unreserved set.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl Render for EnrollmentLink {
    fn render(&self) -> maud::Markup {
        let code = QrCode::new(self.to_uri().as_bytes())
            .expect("an enrollment link is well within QR capacity");
        let svg = code
            .render()
            .min_dimensions(320, 320)
            .dark_color(svg::Color("#000000"))
            .light_color(svg::Color("#ffffff"))
            .build();
        PreEscaped(svg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(url: &str) -> EnrollmentLink {
        EnrollmentLink::new(url, 8089, 8443, "phone-1", "abc123").unwrap()
    }

    #[test]
    fn default_ports_give_the_plain_atak_link() {
        assert_eq!(
            link("https://192.168.1.10:8446").to_uri(),
            "tak://com.atakmap.app/enroll?host=192.168.1.10&username=phone-1&token=abc123"
        );
    }

    #[test]
    fn behind_a_proxy_the_enrollment_port_is_included() {
        assert!(link("https://tak.example.com/").to_uri().ends_with("&enrollmentport=443"));
    }

    #[test]
    fn non_default_streaming_and_api_ports_are_included() {
        let uri = EnrollmentLink::new("https://h:8446", 18089, 18443, "u", "t")
            .unwrap()
            .to_uri();
        assert!(uri.ends_with("&port=18089&apiport=18443"), "{uri}");
    }

    #[test]
    fn values_are_percent_encoded() {
        let uri = EnrollmentLink::new("https://h:8446", 8089, 8443, "team a&b", "t=1")
            .unwrap()
            .to_uri();
        assert!(uri.contains("username=team%20a%26b&token=t%3D1"), "{uri}");
    }

    #[test]
    fn refuses_plain_http_ipv6_and_malformed_urls() {
        assert!(matches!(
            EnrollmentLink::new("http://h:8446", 8089, 8443, "u", "t"),
            Err(LinkError::NotHttps(_))
        ));
        assert_eq!(
            EnrollmentLink::new("https://[fd00::1]:8446", 8089, 8443, "u", "t"),
            Err(LinkError::Ipv6Literal)
        );
        assert!(matches!(
            EnrollmentLink::new("https://h:notaport", 8089, 8443, "u", "t"),
            Err(LinkError::Malformed(_))
        ));
    }

    #[test]
    fn renders_non_empty_svg() {
        let rendered = link("https://192.168.1.10:8446").render().into_string();
        assert!(rendered.contains("<svg"));
    }
}
