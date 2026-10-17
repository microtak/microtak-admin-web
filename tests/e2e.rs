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

async fn enroll(base_url: &str, common_name: &str, token: &str) -> (String, rcgen::KeyPair) {
    let (csr_pem, key) = pki::build_csr(common_name).unwrap();
    // The enrollment port is HTTPS-only with a cert from the server's own
    // CA; like a real device on first contact, trust it and pin the CA
    // afterwards (the tests below get the CA from the running App).
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .no_proxy()
        .build()
        .unwrap();
    let response = client
        .post(format!("{base_url}/Marti/api/tls/signClient/v2?token={token}"))
        .header("Content-Type", "application/octet-stream")
        .body(csr_pem)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: serde_json::Value = response.json().await.unwrap();
    // Real Marti wire format: bare base64, no PEM armor -- a real client
    // (`node-tak`'s `credentials.ts`) re-wraps this itself, so tests do the
    // same rather than assuming full-PEM.
    let bare_cert = body["signedCert"].as_str().unwrap();
    let cert_pem = format!("-----BEGIN CERTIFICATE-----\n{bare_cert}\n-----END CERTIFICATE-----\n");
    (cert_pem, key)
}

/// Sets up a real running `App` with an admin device already enrolled --
/// the way a real operator does it: with the one-time bootstrap token the
/// server writes to its data dir on first start (microtak-server's
/// `src/bootstrap.rs`; the admin CN can't be enrolled any other way) --
/// and a `microtak-admin-web` router pointed at it with that admin's real
/// cert.
async fn setup() -> (axum::Router, Arc<microtak_server::missions::MissionStore>, std::path::PathBuf) {
    let (router, server, creds_dir) = setup_full().await;
    (router, server.missions, creds_dir)
}

/// What the device tests need from the running server.
struct Server {
    missions: Arc<microtak_server::missions::MissionStore>,
    registry: Arc<microtak_server::registry::DeviceRegistry>,
    /// A real certificate for a test device (the registry only needs its
    /// fingerprint, so any CA will do).
    device_cert_pem: String,
}

async fn setup_with_registry() -> (axum::Router, Server, std::path::PathBuf) {
    setup_full().await
}

async fn setup_full() -> (axum::Router, Server, std::path::PathBuf) {
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
    let registry = app.registry.clone();
    let bootstrap_path = app
        .bootstrap_token_path()
        .expect("a fresh server has a bootstrap token outstanding");
    let bootstrap_token = std::fs::read_to_string(bootstrap_path).unwrap();
    tokio::spawn(app.run());

    let enrollment_base_url = format!("https://{enrollment_addr}");
    let (admin_cert, admin_key) =
        enroll(&enrollment_base_url, "web-admin", bootstrap_token.trim()).await;

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
        enrollment_url: Some("https://192.168.1.10:8446".to_string()),
        streaming_port: 8089,
        api_port: 8443,
    };
    let router = pages::router(state);

    let device_ca = pki::CertificateAuthority::generate("Device Test CA").unwrap();
    let (device_csr, _) = pki::build_csr("field-phone").unwrap();
    let device_cert_pem = device_ca
        .sign_csr(&device_csr, time::Duration::days(1))
        .unwrap()
        .cert_pem;
    (
        router,
        Server {
            missions,
            registry,
            device_cert_pem,
        },
        creds_dir,
    )
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
                // What a browser sends for a form submitted from the UI itself.
                .header("host", "admin.local:8090")
                .header("origin", "http://admin.local:8090")
                .header("sec-fetch-site", "same-origin")
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

    // Without a device name: a token, but no QR code (it'd have no
    // username to carry).
    let (status, body) = post_form(&router, "/tokens", "common_name=&expires_in_secs=&note=unbound").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Token minted"));
    assert!(!body.contains("<svg"), "no QR code without a device name");

    // With one: the token is bound to it, and the page carries the
    // standard TAK enrollment link as a QR code and a tap-to-open link.
    let (status, body) = post_form(
        &router,
        "/tokens",
        "common_name=phone-1&expires_in_secs=&note=for+test+device",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<svg"), "expected a real inline QR SVG");
    let link_start = body.find("tak://com.atakmap.app/enroll?").expect("a tak:// enrollment link");
    let link = &body[link_start..body[link_start..].find('"').unwrap() + link_start];
    assert!(
        link.starts_with("tak://com.atakmap.app/enroll?host=192.168.1.10&amp;username=phone-1&amp;token="),
        "{link}"
    );
    assert!(!link.contains("enrollmentport="), "default port 8446 is left out: {link}");

    let (status, body) = get(&router, "/tokens").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("for test device"));
    assert!(body.contains("<td>phone-1</td>"), "the list shows the binding");
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

/// Mission roles through the real web UI against the official mission API:
/// a mission with a default role (the official server only allows role
/// changes there) and a subscriber; promote it through the form, see it,
/// then remove the subscription.
#[tokio::test]
async fn e2e_web_ui_manages_real_mission_roles() {
    let (router, missions, _creds_dir) = setup().await;
    let mission = missions
        .create(
            microtak_server::missions::NewMission {
                guid: "6f1c0000-0000-4000-8000-0000000000aa".into(),
                name: "Web UI Test".into(),
                fields: Default::default(),
                tool: "public".into(),
                creator_uid: "ANDROID-ADMIN".into(),
                groups: ["__ANON__".to_string()].into(),
                password_hash: None,
                default_role: Some(microtak_server::missions::Role::Subscriber),
                invite_only: false,
            },
            1_000,
        )
        .unwrap();
    missions
        .subscribe(&mission.guid, "ANDROID-SECOND", "second-device", microtak_server::missions::Role::Subscriber, 2_000, |id| id.into())
        .unwrap();

    let (status, body) = get(&router, "/missions").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Web UI Test"));

    let (status, _) = post_form(&router, "/missions/Web%20UI%20Test", "uid=ANDROID-SECOND&role=MISSION_OWNER").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, body) = get(&router, "/missions/Web%20UI%20Test").await;
    assert!(body.contains("ANDROID-SECOND") && body.contains("MISSION_OWNER"), "{body}");

    let (status, _) = post_form(&router, "/missions/Web%20UI%20Test/roles/ANDROID-SECOND/revoke", "").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, body) = get(&router, "/missions/Web%20UI%20Test").await;
    assert!(!body.contains("ANDROID-SECOND"), "the subscription is gone: {body}");
}

/// Group management through the real web UI against a real server: create
/// a group, add a member with a direction, see both, mint a token that
/// carries the group, remove the member, delete the group.
#[tokio::test]
async fn e2e_web_ui_manages_real_groups() {
    let (router, _app, _creds_dir) = setup().await;

    let (status, _) = post_form(&router, "/groups", "name=Red&description=red+team").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _) = post_form(&router, "/groups/Red/members", "identity=phone-1&direction=OUT").await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, body) = get(&router, "/groups").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<h2>Red"), "{body}");
    assert!(body.contains("red team"));
    assert!(body.contains("<td>phone-1</td><td>out</td>"), "{body}");

    // A token carrying the group, and the token list showing it.
    let (status, _) = post_form(&router, "/tokens", "common_name=phone-2&groups=Red&groups_in=&groups_out=&expires_in_secs=&note=").await;
    assert_eq!(status, StatusCode::OK);
    let (_, body) = get(&router, "/tokens").await;
    assert!(body.contains("<td>Red</td>"), "{body}");

    // An unknown group surfaces as a readable error, not a panic.
    let (status, body) = post_form(&router, "/tokens", "common_name=phone-3&groups=Nope").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("unknown group"), "{body}");

    let (status, _) = post_form(&router, "/groups/Red/members/phone-1/remove", "").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (status, _) = post_form(&router, "/groups/Red/delete", "").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let (_, body) = get(&router, "/groups").await;
    assert!(!body.contains("<h2>Red"));
}

/// A POST with arbitrary extra headers, e.g. what a browser sends when a
/// third-party page submits a form to this UI.
async fn post_form_with(router: &axum::Router, uri: &str, form: &str, headers: &[(&str, &str)]) -> StatusCode {
    let mut request = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("host", "admin.local:8090");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    router
        .clone()
        .oneshot(request.body(Body::from(form.to_string())).unwrap())
        .await
        .unwrap()
        .status()
}

/// CSRF: the browser sends the UI's Basic Auth credentials with any form
/// post, also one a malicious page makes it submit. Such posts are refused
/// and change nothing on the server; posts from the UI itself work.
#[tokio::test]
async fn e2e_cross_site_form_posts_are_refused() {
    let (router, _app, _creds_dir) = setup().await;
    let cross_site: &[&[(&str, &str)]] = &[
        &[("origin", "https://evil.example"), ("sec-fetch-site", "cross-site")],
        &[("origin", "https://evil.example")],
        &[("referer", "https://evil.example/page")],
        &[("sec-fetch-site", "same-site"), ("origin", "http://other.admin.local:8090")],
        &[], // no provenance at all
    ];
    for headers in cross_site {
        let status = post_form_with(&router, "/tokens", "", headers).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "headers {headers:?}");
    }
    let (_, page) = get(&router, "/tokens").await;
    assert!(!page.contains("<td><code>"), "nothing was minted: {page}");

    for headers in [
        &[("origin", "http://admin.local:8090")][..],
        &[("referer", "http://admin.local:8090/tokens")][..],
        &[("sec-fetch-site", "same-origin")][..],
    ] {
        let status = post_form_with(&router, "/tokens", "", headers).await;
        assert_eq!(status, StatusCode::OK, "headers {headers:?}");
    }
}

/// Devices page: lists the issued certificates (official certificate
/// admin API), revokes one by hash, blocks and unblocks the identity --
/// each reflected on the real server.
#[tokio::test]
async fn e2e_web_ui_revokes_certificates_and_blocks_identities() {
    let (router, app, _creds_dir) = setup_with_registry().await;
    app.registry.enroll("field-phone", &app.device_cert_pem, 1).unwrap();
    let hash = pki::fingerprint_sha256_colon_hex(&app.device_cert_pem).unwrap();

    let (status, page) = get(&router, "/devices").await;
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("field-phone") && page.contains("valid"), "{page}");

    let encoded: String = hash.replace(':', "%3A");
    let (status, _) = post_form(&router, &format!("/devices/certificates/{encoded}/revoke"), "").await;
    assert!(status.is_success() || status.is_redirection(), "{status}");
    assert!(app.registry.is_cert_revoked(&hash));
    let (_, page) = get(&router, "/devices").await;
    assert!(page.contains("revoked"), "{page}");

    let (status, _) = post_form(&router, "/devices/field-phone/block", "").await;
    assert!(status.is_success() || status.is_redirection(), "{status}");
    assert!(app.registry.is_revoked("field-phone"));
    let (status, _) = post_form(&router, "/devices/field-phone/unblock", "").await;
    assert!(status.is_success() || status.is_redirection(), "{status}");
    assert!(!app.registry.is_revoked("field-phone"));
}
