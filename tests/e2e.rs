//! End-to-end test: drives `microtak-admin-web`'s real router (via
//! `tower::ServiceExt::oneshot`, the same pattern `microtak-server`'s own
//! `src/marti/*.rs` module tests use) against a real, running
//! `microtak_server::app::App` -- not a mock. Enrolls a real admin device
//! against the real server first (the actual documented bootstrap flow),
//! then drives the web UI's own HTTP routes to mint a token and assign a
//! mission role, confirming each is actually reflected on the real server.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use microtak_admin_web::client::MicrotakClient;
use microtak_admin_web::pages::{self, AppState};
use microtak_server::app::{App, AppConfig};
use microtak_server::pki;
use tower::ServiceExt;

const SERVER_NAME: &str = "microtak-server";

static TEST_DIR_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    let n = TEST_DIR_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("microtak-admin-web-e2e-{label}-{}-{n}", std::process::id()))
}

async fn enroll(base_url: &str, common_name: &str) -> (String, rcgen::KeyPair) {
    let (csr_pem, key) = pki::build_csr(common_name).unwrap();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{base_url}/Marti/api/tls/signClient/v2"))
        .header("Content-Type", "application/octet-stream")
        .body(csr_pem)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    let cert_pem = body["signedCert"].as_str().unwrap().to_string();
    (cert_pem, key)
}

/// Sets up a real running `App` with an admin device already enrolled
/// (mirroring the documented two-phase bootstrap: enroll while open, then
/// -- here, simply configuring `admin_common_name` from the start works
/// fine since we don't also need `enrollment_requires_token` for this
/// test), and a `microtak-admin-web` router pointed at it with that
/// admin's real cert.
async fn setup() -> (axum::Router, Arc<microtak_server::missions::MissionStore>, std::path::PathBuf) {
    let creds_dir = unique_temp_dir("creds");
    std::fs::create_dir_all(&creds_dir).unwrap();

    let config = AppConfig {
        enrollment_addr: "127.0.0.1:0".parse().unwrap(),
        marti_api_addr: "127.0.0.1:0".parse().unwrap(),
        plain_tcp_addr: "127.0.0.1:0".parse().unwrap(),
        mtls_addr: "127.0.0.1:0".parse().unwrap(),
        data_dir: unique_temp_dir("data"),
        admin_common_name: Some("web-admin".to_string()),
        ..AppConfig::default()
    };
    let app = App::bind(config).await.unwrap();
    let enrollment_addr = app.enrollment_addr().unwrap();
    let marti_api_addr = app.marti_api_addr().unwrap();
    let ca_cert_pem = app.ca_cert_pem.clone();
    let missions = app.missions.clone();
    tokio::spawn(app.run());

    let enrollment_base_url = format!("http://{enrollment_addr}");
    let (admin_cert, admin_key) = enroll(&enrollment_base_url, "web-admin").await;

    let cert_path = creds_dir.join("admin.pem");
    let key_path = creds_dir.join("admin.key");
    let ca_path = creds_dir.join("ca.pem");
    std::fs::write(&cert_path, &admin_cert).unwrap();
    std::fs::write(&key_path, admin_key.serialize_pem()).unwrap();
    std::fs::write(&ca_path, &ca_cert_pem).unwrap();

    let base_url = format!("https://{SERVER_NAME}:{}", marti_api_addr.port());
    let client = MicrotakClient::new_with_resolve_override(
        &base_url,
        &cert_path,
        &key_path,
        &ca_path,
        SERVER_NAME,
        SocketAddr::new(marti_api_addr.ip(), marti_api_addr.port()),
    )
    .unwrap();

    let state = AppState {
        client: Arc::new(client),
        enrollment_url: None,
    };
    let router = pages::router(state);

    (router, missions, creds_dir)
}

async fn get(router: &axum::Router, uri: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn post_form(router: &axum::Router, uri: &str, form: &str) -> (StatusCode, String) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

/// The core token-management flow, through the real web UI, against a real
/// running server: the list starts empty, minting via the form works and
/// shows the token + a real QR SVG, the list then shows it, and revoking
/// via the form actually removes its "revoke" action (confirming the
/// server-side state genuinely changed, not just that the page reloaded).
#[tokio::test]
async fn e2e_web_ui_mints_lists_and_revokes_a_real_token() {
    let (router, _app, _creds_dir) = setup().await;

    let (status, body) = get(&router, "/tokens").await;
    assert_eq!(status, StatusCode::OK);
    assert!(!body.contains("<code>"), "no tokens minted yet");

    let (status, body) = post_form(&router, "/tokens", "expires_in_secs=&note=for+test+device").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Token minted"));
    assert!(body.contains("<svg"), "expected a real inline QR SVG");

    let (status, body) = get(&router, "/tokens").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("for test device"));
    assert!(body.contains("Revoke"));

    // Extract the token value from the raw href the list rendered so we
    // can revoke the exact real token, not a guessed one.
    let marker = "/tokens/";
    let start = body.find(marker).unwrap() + marker.len();
    let end = body[start..].find("/revoke").unwrap() + start;
    let token = &body[start..end];

    let (status, _) = post_form(&router, &format!("/tokens/{token}/revoke"), "").await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, body) = get(&router, "/tokens").await;
    assert!(body.contains("revoked"));
}

/// Mission role management through the real web UI: create a mission
/// directly against the real server (the web UI doesn't create missions,
/// only manages roles on existing ones), assign a role to a second
/// identity through the form and confirm it's reflected, then confirm the
/// last-owner protection's 409 surfaces as a real, readable error in the
/// UI rather than a panic.
///
/// Note on scenario shape: every request through this UI is authenticated
/// as the single configured admin identity (`web-admin`) -- so demoting
/// *that* identity's own Owner role mid-test would correctly cost it the
/// authorization to make any further role-management calls at all (a real
/// thing this test found by trying it naively first). The valid way to
/// exercise the last-owner protection here is to attempt revoking
/// `web-admin`'s own role while it's still the mission's *sole* owner --
/// the mission was never given a second owner, so this must be rejected.
#[tokio::test]
async fn e2e_web_ui_manages_real_mission_roles() {
    let (router, missions, _creds_dir) = setup().await;

    missions
        .create("Web UI Test", None, "web-admin", vec![], 1_000)
        .unwrap();

    let (status, body) = get(&router, "/missions").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Web UI Test"));

    // Positive case: assign a Subscriber role to a second identity through
    // the real form and confirm it's actually reflected by the server.
    let (status, _) = post_form(
        &router,
        "/missions/Web%20UI%20Test",
        "uid=second-device&role=subscriber",
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, body) = get(&router, "/missions/Web%20UI%20Test").await;
    assert!(body.contains("second-device"));
    assert!(body.contains("subscriber"));

    // Negative case: web-admin is still the mission's *sole* owner --
    // revoking its own role must be rejected (the last-owner protection),
    // and the UI must show a real, readable error, not panic or silently
    // redirect as if it worked.
    let (status, body) = post_form(&router, "/missions/Web%20UI%20Test/roles/web-admin/revoke", "").await;
    assert_eq!(status, StatusCode::OK, "expected an in-page error, not a redirect");
    assert!(body.contains("last remaining owner"), "got: {body}");

    // Confirm it genuinely wasn't revoked, not just that the page said so.
    let (_, body) = get(&router, "/missions/Web%20UI%20Test").await;
    assert!(body.contains("web-admin"));
}
