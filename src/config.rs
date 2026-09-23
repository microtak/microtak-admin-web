//! Configuration, entirely via environment variables -- no CLI flag parser
//! dependency needed for a handful of required paths/URLs.

use std::net::SocketAddr;
use std::path::PathBuf;

/// Everything needed to run `microtak-admin-web`. See `README.md` for the
/// exact environment variable names and what each one means.
pub struct Config {
    /// The target microtak-server's Marti API base URL, e.g.
    /// `https://microtak.example.com:8443`. Must match a DNS name (or
    /// `--resolve`-style override, not supported outside tests) covered by
    /// the server's own TLS certificate SAN.
    pub server_url: String,
    pub admin_cert_path: PathBuf,
    pub admin_key_path: PathBuf,
    pub ca_cert_path: PathBuf,
    /// Where `microtak-admin-web` itself listens.
    pub bind_addr: SocketAddr,
    /// Shared secret checked via HTTP Basic Auth on every page -- see
    /// `src/auth.rs`'s own doc comment for why this is a deliberate v1
    /// simplification, not a finished access-control system.
    pub web_password: String,
    /// Optional: the microtak-server's *enrollment* endpoint base URL
    /// (different port than the Marti API, plain HTTP, unauthenticated --
    /// e.g. `http://microtak.example.com:8446`). If set, minted tokens'
    /// QR codes include it so a provisioning client knows where to enroll,
    /// not just the bare token value.
    pub enrollment_url: Option<String>,
}

impl Config {
    /// Reads all required variables, panicking with a clear message naming
    /// the missing one -- matching `microtak-server`'s own `main.rs` style
    /// of failing loudly and immediately on bad startup config rather than
    /// deferring to first use.
    pub fn from_env() -> Self {
        Self {
            server_url: require_env("MICROTAK_ADMIN_WEB_SERVER"),
            admin_cert_path: PathBuf::from(require_env("MICROTAK_ADMIN_WEB_CERT")),
            admin_key_path: PathBuf::from(require_env("MICROTAK_ADMIN_WEB_KEY")),
            ca_cert_path: PathBuf::from(require_env("MICROTAK_ADMIN_WEB_CA")),
            bind_addr: std::env::var("MICROTAK_ADMIN_WEB_BIND")
                .unwrap_or_else(|_| "127.0.0.1:8090".to_string())
                .parse()
                .unwrap_or_else(|error| panic!("invalid MICROTAK_ADMIN_WEB_BIND: {error}")),
            web_password: require_env("MICROTAK_ADMIN_WEB_PASSWORD"),
            enrollment_url: std::env::var("MICROTAK_ADMIN_WEB_ENROLLMENT_URL").ok(),
        }
    }
}

fn require_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| {
        panic!("missing required environment variable {name} -- see README.md for what to set")
    })
}
