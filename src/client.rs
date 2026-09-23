//! A thin client for the pieces of microtak-server's mTLS Marti API this
//! tool needs: enrollment token admin endpoints and mission role
//! management. Deliberately not a dependency on the `microtak-server`
//! crate itself (that stays a *dev*-dependency, only for the end-to-end
//! test) -- this talks to a real, possibly-remote server purely over HTTP,
//! the same way any other Marti API client would.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("request to microtak-server failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("microtak-server rejected the request: {status} {body}")]
    Rejected {
        status: reqwest::StatusCode,
        body: String,
    },
    #[error("failed to read cert/key/CA file: {0}")]
    Io(#[from] std::io::Error),
}

/// Mirrors `microtak_server::enrollment_tokens::EnrollmentToken` field-for-
/// field (plain snake_case JSON, no rename on that struct upstream) --
/// duplicated here rather than imported from the crate, since this binary
/// has no runtime dependency on it (see this module's own doc comment).
#[derive(Debug, Clone, Deserialize)]
pub struct EnrollmentToken {
    pub token: String,
    pub created_at_unix: i64,
    pub expires_at_unix: Option<i64>,
    pub note: Option<String>,
    pub used: bool,
    pub used_by_common_name: Option<String>,
    #[allow(dead_code)]
    pub used_at_unix: Option<i64>,
    pub revoked: bool,
}

/// Mirrors `microtak_server::missions::Mission` -- see the note on
/// `EnrollmentToken` above.
#[derive(Debug, Clone, Deserialize)]
pub struct Mission {
    pub name: String,
    #[allow(dead_code)]
    pub description: Option<String>,
    pub creator_uid: String,
    pub roles: std::collections::BTreeMap<String, String>,
}

#[derive(Serialize)]
struct MintTokenRequest {
    #[serde(rename = "expiresInSecs", skip_serializing_if = "Option::is_none")]
    expires_in_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Deserialize)]
struct MintTokenResponse {
    token: String,
}

#[derive(Serialize)]
struct AssignRoleRequest<'a> {
    uid: &'a str,
    role: &'a str,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    error: String,
}

pub struct MicrotakClient {
    http: reqwest::Client,
    base_url: String,
}

impl MicrotakClient {
    pub fn new(
        base_url: &str,
        cert_path: &Path,
        key_path: &Path,
        ca_path: &Path,
    ) -> Result<Self, ClientError> {
        let mut identity_pem = std::fs::read(cert_path)?;
        identity_pem.extend_from_slice(&std::fs::read(key_path)?);
        let identity = reqwest::Identity::from_pem(&identity_pem)?;
        let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(ca_path)?)?;

        let http = reqwest::Client::builder()
            .identity(identity)
            .add_root_certificate(ca_cert)
            .build()?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// Test-only constructor: real deployments connect to a real DNS name
    /// matching the server's cert SAN, but this project's own test servers
    /// (including microtak-server's) use a fixed SAN hostname on a loopback
    /// IP -- `resolve_override` reproduces the same `.resolve()` trick
    /// `microtak-server`'s own `tests/e2e.rs` uses, so this client can be
    /// exercised against a real, running test server without needing real
    /// DNS.
    // Not `#[cfg(test)]`: it needs to stay compiled into the library so
    // `tests/e2e.rs` (an external integration test binary, which never sees
    // `#[cfg(test)]` items from the lib it links against) can call it.
    pub fn new_with_resolve_override(
        base_url: &str,
        cert_path: &Path,
        key_path: &Path,
        ca_path: &Path,
        server_name: &str,
        addr: std::net::SocketAddr,
    ) -> Result<Self, ClientError> {
        let mut identity_pem = std::fs::read(cert_path)?;
        identity_pem.extend_from_slice(&std::fs::read(key_path)?);
        let identity = reqwest::Identity::from_pem(&identity_pem)?;
        let ca_cert = reqwest::Certificate::from_pem(&std::fs::read(ca_path)?)?;

        let http = reqwest::Client::builder()
            .identity(identity)
            .add_root_certificate(ca_cert)
            .resolve(server_name, std::net::SocketAddr::new(addr.ip(), 0))
            .no_proxy()
            .build()?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    pub async fn list_tokens(&self) -> Result<Vec<EnrollmentToken>, ClientError> {
        let response = self
            .http
            .get(format!("{}/Marti/api/admin/enrollmentTokens", self.base_url))
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    pub async fn mint_token(
        &self,
        expires_in_secs: Option<i64>,
        note: Option<String>,
    ) -> Result<String, ClientError> {
        let response = self
            .http
            .post(format!("{}/Marti/api/admin/enrollmentTokens", self.base_url))
            .json(&MintTokenRequest {
                expires_in_secs,
                note,
            })
            .send()
            .await?;
        let response = check_status(response).await?;
        let body: MintTokenResponse = response.json().await?;
        Ok(body.token)
    }

    pub async fn revoke_token(&self, token: &str) -> Result<(), ClientError> {
        let response = self
            .http
            .delete(format!(
                "{}/Marti/api/admin/enrollmentTokens/{token}",
                self.base_url
            ))
            .send()
            .await?;
        check_status(response).await?;
        Ok(())
    }

    pub async fn list_missions(&self) -> Result<Vec<Mission>, ClientError> {
        let response = self
            .http
            .get(format!("{}/Marti/api/missions", self.base_url))
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    pub async fn get_mission(&self, name: &str) -> Result<Option<Mission>, ClientError> {
        let response = self
            .http
            .get(format!(
                "{}/Marti/api/missions/{}",
                self.base_url,
                urlencoding_light(name)
            ))
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = check_status(response).await?;
        Ok(Some(response.json().await?))
    }

    pub async fn assign_role(&self, mission: &str, uid: &str, role: &str) -> Result<(), ClientError> {
        let response = self
            .http
            .put(format!(
                "{}/Marti/api/missions/{}/role",
                self.base_url,
                urlencoding_light(mission)
            ))
            .json(&AssignRoleRequest { uid, role })
            .send()
            .await?;
        check_status(response).await?;
        Ok(())
    }

    pub async fn revoke_role(&self, mission: &str, uid: &str) -> Result<(), ClientError> {
        let response = self
            .http
            .delete(format!(
                "{}/Marti/api/missions/{}/role/{}",
                self.base_url,
                urlencoding_light(mission),
                urlencoding_light(uid)
            ))
            .send()
            .await?;
        check_status(response).await?;
        Ok(())
    }
}

async fn check_status(response: reqwest::Response) -> Result<reqwest::Response, ClientError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response
        .json::<ApiErrorBody>()
        .await
        .map(|b| b.error)
        .unwrap_or_else(|_| "(no error detail)".to_string());
    Err(ClientError::Rejected { status, body })
}

/// A minimal percent-encoder for path segments -- mission names can
/// contain spaces/`%`/`/` (see microtak-server's own TC-MARTI-03), and
/// this avoids pulling in a whole URL-encoding crate for the handful of
/// characters that actually show up in practice.
fn urlencoding_light(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
