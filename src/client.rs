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
    /// The device name the token is bound to, if any.
    #[serde(default)]
    pub common_name: Option<String>,
    /// Groups the enrolling device is added to.
    #[serde(default)]
    pub groups: Vec<GroupGrant>,
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
    /// The groups the mission is visible in (older servers omit this).
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(rename = "defaultRole", default)]
    pub default_role: Option<String>,
}

/// A group membership handed out at enrollment.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct GroupGrant {
    pub name: String,
    /// `IN`, `OUT` or `BOTH`.
    pub membership: String,
}

/// Short text form of grants: `Red, Blue(out)`.
pub fn describe_grants(grants: &[GroupGrant]) -> String {
    if grants.is_empty() {
        return "—".to_string();
    }
    grants
        .iter()
        .map(|g| match g.membership.as_str() {
            "BOTH" => g.name.clone(),
            other => format!("{}({})", g.name, other.to_lowercase()),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Groups to put an enrolling device into -- the official user-file lists.
#[derive(Serialize, Default, Debug, PartialEq)]
pub struct GroupLists {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    #[serde(rename = "groupsIn", skip_serializing_if = "Vec::is_empty")]
    pub groups_in: Vec<String>,
    #[serde(rename = "groupsOut", skip_serializing_if = "Vec::is_empty")]
    pub groups_out: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct GroupInfo {
    pub name: String,
    pub description: Option<String>,
    pub bitpos: u32,
    /// identity -> `IN` / `OUT` / `BOTH`
    pub members: std::collections::BTreeMap<String, String>,
}

#[derive(Serialize)]
struct MintTokenRequest {
    #[serde(rename = "expiresInSecs", skip_serializing_if = "Option::is_none")]
    expires_in_secs: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
    #[serde(rename = "commonName", skip_serializing_if = "Option::is_none")]
    common_name: Option<String>,
    #[serde(flatten)]
    groups: GroupLists,
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
        common_name: Option<String>,
        groups: GroupLists,
    ) -> Result<String, ClientError> {
        let response = self
            .http
            .post(format!("{}/Marti/api/admin/enrollmentTokens", self.base_url))
            .json(&MintTokenRequest {
                expires_in_secs,
                note,
                common_name,
                groups,
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

    pub async fn list_groups(&self) -> Result<Vec<GroupInfo>, ClientError> {
        let response = self
            .http
            .get(format!("{}/Marti/api/admin/groups", self.base_url))
            .send()
            .await?;
        let response = check_status(response).await?;
        Ok(response.json().await?)
    }

    pub async fn create_group(&self, name: &str, description: Option<String>) -> Result<(), ClientError> {
        let response = self
            .http
            .post(format!("{}/Marti/api/admin/groups", self.base_url))
            .json(&serde_json::json!({ "name": name, "description": description }))
            .send()
            .await?;
        check_status(response).await?;
        Ok(())
    }

    pub async fn delete_group(&self, name: &str) -> Result<(), ClientError> {
        let response = self
            .http
            .delete(format!("{}/Marti/api/admin/groups/{}", self.base_url, urlencoding_light(name)))
            .send()
            .await?;
        check_status(response).await?;
        Ok(())
    }

    /// `direction`: `IN`, `OUT` or `BOTH`.
    pub async fn set_group_member(&self, group: &str, identity: &str, direction: &str) -> Result<(), ClientError> {
        let response = self
            .http
            .put(format!(
                "{}/Marti/api/admin/groups/{}/members/{}",
                self.base_url,
                urlencoding_light(group),
                urlencoding_light(identity)
            ))
            .json(&serde_json::json!({ "direction": direction }))
            .send()
            .await?;
        check_status(response).await?;
        Ok(())
    }

    pub async fn remove_group_member(&self, group: &str, identity: &str) -> Result<(), ClientError> {
        let response = self
            .http
            .delete(format!(
                "{}/Marti/api/admin/groups/{}/members/{}",
                self.base_url,
                urlencoding_light(group),
                urlencoding_light(identity)
            ))
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
