//! All HTML pages: server-rendered with `maud`, forms submit as plain
//! `application/x-www-form-urlencoded` POSTs -- no client-side JS framework.
//! Styling is Tailwind, compiled ahead of time into a committed bundle
//! (`assets/app.css`) that is embedded in the binary and inlined into every
//! page, so the runtime stays build-free, dependency-free and offline (no
//! CDN) -- important for the field tablet. See `CSS` below to regenerate.

use std::sync::Arc;

use axum::Router;
use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use maud::{DOCTYPE, Markup, html};
use serde::Deserialize;

use crate::client::{ClientError, MicrotakClient};
use crate::qr::EnrollmentLink;

#[derive(Clone)]
pub struct AppState {
    pub client: Arc<MicrotakClient>,
    /// Where devices reach the server's (HTTPS-only) enrollment endpoint --
    /// the QR code's `host`/`enrollmentport`. No QR codes without it.
    pub enrollment_url: Option<String>,
    /// The streaming (mTLS CoT) and Marti API ports devices connect to.
    pub streaming_port: u16,
    pub api_port: u16,
    /// Base URL of the server's private Prometheus endpoint, if enabled --
    /// drives the "MicroTAK server" panel on the System page.
    pub metrics_url: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(overview_page))
        .route("/tokens", get(list_tokens_page).post(mint_token_page))
        .route("/tokens/:token/revoke", post(revoke_token_page))
        .route("/users", get(list_users_page).post(mint_user_page))
        .route("/users/:username/revoke", post(revoke_user_page))
        .route("/groups", get(list_groups_page).post(create_group_page))
        .route("/groups/:name/delete", post(delete_group_page))
        .route("/groups/:name/members", post(set_member_page))
        .route(
            "/groups/:name/members/:identity/remove",
            post(remove_member_page),
        )
        .route(
            "/missions",
            get(list_missions_page).post(create_mission_page),
        )
        .route(
            "/missions/:name",
            get(mission_detail_page).post(assign_role_page),
        )
        .route("/missions/:name/roles/:uid/revoke", post(revoke_role_page))
        .route("/system", get(system_page))
        .route("/devices", get(list_devices_page))
        .route(
            "/devices/certificates/:hash/revoke",
            post(revoke_certificate_page),
        )
        .route("/devices/:name/block", post(block_device_page))
        .route("/devices/:name/unblock", post(unblock_device_page))
        .layer(axum::middleware::from_fn(crate::csrf::require_same_origin))
        .with_state(state)
}

fn layout(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { "MicroTAK — " (title) }
                // Set the theme before first paint (no flash): a saved choice
                // wins, else dark between 18:00 and 07:00 local. Defines the
                // toggle used by the header button. Pure inline JS, no framework.
                script { (maud::PreEscaped(THEME_JS)) }
                style { (maud::PreEscaped(CSS)) }
            }
            body class="min-h-screen bg-slate-100 font-sans text-slate-800 antialiased dark:bg-slate-900 dark:text-slate-200" {
                header class="bg-slate-900 text-slate-100 shadow-md dark:bg-slate-950" {
                    div class="mx-auto flex max-w-5xl items-center gap-3 px-4 py-3" {
                        span class="h-2.5 w-2.5 rounded-full bg-emerald-400" {}
                        span class="text-lg font-semibold tracking-tight" { "MicroTAK" }
                        span class="hidden text-sm text-slate-400 sm:inline" { "admin" }
                        nav class="ml-auto flex flex-wrap items-center gap-1" {
                            a class="nav-link" href="/" { "Overview" }
                            a class="nav-link" href="/tokens" { "Tokens" }
                            a class="nav-link" href="/users" { "Users" }
                            a class="nav-link" href="/missions" { "Missions" }
                            a class="nav-link" href="/groups" { "Groups" }
                            a class="nav-link" href="/devices" { "Devices" }
                            a class="nav-link" href="/system" { "System" }
                            button type="button" class="theme-toggle" onclick="mtToggleTheme()" title="Toggle light / dark" aria-label="Toggle light or dark theme" { "◐" }
                        }
                    }
                }
                main class="mx-auto max-w-5xl px-4 py-8" {
                    h1 class="mb-6 text-2xl font-bold tracking-tight text-slate-900 dark:text-slate-100" { (title) }
                    (body)
                }
            }
        }
    }
}

// Tailwind-generated bundle, embedded so the admin UI needs no CDN and works
// offline on the field tablet. Regenerate after changing markup classes:
//   tailwindcss -c tailwind.config.js -i assets/tailwind.css -o assets/app.css --minify
// (standalone CLI or `npx tailwindcss`). The output is committed; the runtime
// still has no build step.
const CSS: &str = include_str!("../assets/app.css");

// Minimal theme bootstrap: apply before first paint, default by time of day,
// remember an explicit choice. `mtToggleTheme` is wired to the header button.
const THEME_JS: &str = r#"(function(){
  function apply(t){document.documentElement.classList.toggle('dark',t==='dark');}
  function pick(){
    var s=null;try{s=localStorage.getItem('mt-theme');}catch(e){}
    if(s==='dark'||s==='light')return s;
    var h=new Date().getHours();return (h>=18||h<7)?'dark':'light';
  }
  apply(pick());
  window.mtToggleTheme=function(){
    var next=document.documentElement.classList.contains('dark')?'light':'dark';
    try{localStorage.setItem('mt-theme',next);}catch(e){}
    apply(next);
  };
})();"#;

fn error_banner(message: &str) -> Markup {
    html! { div class="error" { (message) } }
}

fn error_response(error: &ClientError) -> Response {
    error_page(&error.to_string())
}

fn error_page(message: &str) -> Response {
    Html(layout("Error", error_banner(message)).into_string()).into_response()
}

// ---------------------------------------------------------------------
// Enrollment tokens
// ---------------------------------------------------------------------

async fn list_tokens_page(State(state): State<AppState>) -> Response {
    let tokens = match state.client.list_tokens().await {
        Ok(tokens) => tokens,
        Err(error) => return error_response(&error),
    };
    // The group pickers are a convenience; if the groups can't be fetched we
    // still render the form (the fields accept typed names too).
    let groups = state.client.list_groups().await.unwrap_or_default();
    Html(layout("Enrollment Tokens", render_tokens_list(&tokens, &groups)).into_string())
        .into_response()
}

fn render_tokens_list(
    tokens: &[crate::client::EnrollmentToken],
    groups: &[crate::client::GroupInfo],
) -> Markup {
    html! {
        fieldset {
            legend { "Mint a new token" }
            form method="post" action="/tokens" {
                label { "Device name: " input type="text" name="common_name" placeholder="e.g. phone-1"; }
                small { " — binds the token to this identity; needed for the QR code" }
                br;
                // Shared dropdown of the server's existing groups: each field
                // below is a comma-separated list, and `list=` turns it into a
                // pick-from-known-groups box (typing a new name still works).
                datalist id="known-groups" {
                    @for group in groups {
                        option value=(group.name);
                    }
                }
                label { "Groups, both ways (optional): " input type="text" name="groups" list="known-groups" placeholder="Red, Blue"; }
                br;
                label { "Send-only groups (IN, optional): " input type="text" name="groups_in" list="known-groups"; }
                br;
                label { "Receive-only groups (OUT, optional): " input type="text" name="groups_out" list="known-groups"; }
                br;
                label { "Expires in (seconds, optional): " input type="number" name="expires_in_secs"; }
                br;
                label { "Note (optional): " input type="text" name="note"; }
                br;
                button type="submit" { "Mint token" }
            }
        }
        table {
            tr { th{"Token"} th{"For"} th{"Groups"} th{"Note"} th{"Created"} th{"Expires"} th{"Status"} th{} }
            @for token in tokens {
                tr {
                    td { code { (short(&token.token)) } }
                    td { (token.common_name.as_deref().unwrap_or("any")) }
                    td { (crate::client::describe_grants(&token.groups)) }
                    td { (token.note.as_deref().unwrap_or("—")) }
                    td { (token.created_at_unix) }
                    td { (token.expires_at_unix.map(|t| t.to_string()).unwrap_or_else(|| "never".to_string())) }
                    td class=(status_class(token)) { (status_text(token)) }
                    td {
                        @if !token.used && !token.revoked {
                            form class="inline" method="post" action=(format!("/tokens/{}/revoke", token.token)) {
                                button type="submit" class="danger" { "Revoke" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn status_class(token: &crate::client::EnrollmentToken) -> &'static str {
    if token.revoked {
        "revoked"
    } else if token.used {
        "used"
    } else {
        ""
    }
}

fn status_text(token: &crate::client::EnrollmentToken) -> String {
    if token.revoked {
        "revoked".to_string()
    } else if token.used {
        format!(
            "used by {}",
            token.used_by_common_name.as_deref().unwrap_or("?")
        )
    } else {
        "unused".to_string()
    }
}

fn short(token: &str) -> String {
    if token.len() > 16 {
        format!("{}…{}", &token[..8], &token[token.len() - 8..])
    } else {
        token.to_string()
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A compact "time since" label, e.g. `just now`, `5m ago`, `3h ago`, `2d ago`.
fn ago(now: i64, then: i64) -> String {
    let secs = (now - then).max(0);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86400)
    }
}

#[derive(Deserialize)]
struct MintTokenForm {
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    common_name: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    expires_in_secs: Option<i64>,
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    note: Option<String>,
    #[serde(default)]
    groups: String,
    #[serde(default)]
    groups_in: String,
    #[serde(default)]
    groups_out: String,
}

/// A comma-separated form field -> trimmed, non-empty names.
fn name_list(field: &str) -> Vec<String> {
    field
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    match raw.as_deref() {
        None | Some("") => Ok(None),
        Some(value) => value.parse().map(Some).map_err(serde::de::Error::custom),
    }
}

fn empty_string_as_none_str<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.filter(|s| !s.is_empty()))
}

async fn mint_token_page(
    State(state): State<AppState>,
    Form(form): Form<MintTokenForm>,
) -> Response {
    // Validate what the QR code needs before minting, so a bad config
    // doesn't leave an orphaned token behind.
    if let (Some(url), Some(_)) = (&state.enrollment_url, &form.common_name)
        && let Err(error) = EnrollmentLink::new(url, state.streaming_port, state.api_port, "x", "x")
    {
        return error_page(&format!("Can't build an enrollment QR code: {error}"));
    }
    match state
        .client
        .mint_token(
            form.expires_in_secs,
            form.note,
            form.common_name.clone(),
            crate::client::GroupLists {
                groups: name_list(&form.groups),
                groups_in: name_list(&form.groups_in),
                groups_out: name_list(&form.groups_out),
            },
        )
        .await
    {
        Ok(token) => {
            let link = match (&state.enrollment_url, &form.common_name) {
                (Some(url), Some(name)) => {
                    EnrollmentLink::new(url, state.streaming_port, state.api_port, name, &token)
                        .ok()
                }
                _ => None,
            };
            let body = html! {
                p { "Token minted. This is the only time the full value is shown here:" }
                p { code { (token) } }
                @if let Some(link) = &link {
                    p { "Scan with the TAK client (ATAK, OmniTAK, …) on the device — it enrolls and connects on its own. The token works once, for " strong { (link.username) } " only:" }
                    div { (link) }
                    p { "On the device itself, tap instead: " a href=(link.to_uri()) { "open in TAK client" } }
                    p { small { "Anyone who sees this code before the device uses it can enroll as that device — show it only to its user." } }
                } @else if form.common_name.is_none() {
                    p { "No QR code: enter a device name to get one (the token is bound to it)." }
                } @else {
                    p { "No QR code: set MICROTAK_ADMIN_WEB_ENROLLMENT_URL (where devices reach the server) to get one." }
                }
                p { a href="/tokens" { "Back to token list" } }
            };
            Html(layout("Token Minted", body).into_string()).into_response()
        }
        Err(error) => error_response(&error),
    }
}

async fn revoke_token_page(State(state): State<AppState>, Path(token): Path<String>) -> Response {
    match state.client.revoke_token(&token).await {
        Ok(()) => Redirect::to("/tokens").into_response(),
        Err(error) => error_response(&error),
    }
}

// ---------------------------------------------------------------------
// Password accounts
// ---------------------------------------------------------------------

async fn list_users_page(State(state): State<AppState>) -> Response {
    let users = match state.client.list_users().await {
        Ok(users) => users,
        Err(error) => return error_response(&error),
    };
    // As on the token page: the group pickers are only a convenience.
    let groups = state.client.list_groups().await.unwrap_or_default();
    Html(layout("Users", render_users(&users, &groups)).into_string()).into_response()
}

fn render_users(
    users: &[crate::client::UserAccount],
    groups: &[crate::client::GroupInfo],
) -> Markup {
    html! {
        p {
            "Password accounts, for clients that log in with a username and password (e.g. CloudTAK) "
            "rather than an invite token. A device can also enroll with them, any number of times; "
            "the username becomes its certificate name. " strong { "Revoke" } " stops the account "
            "logging in or enrolling and also revokes its device's certificate."
        }
        fieldset {
            legend { "Create an account" }
            form method="post" action="/users" {
                label { "Username: " input type="text" name="username" required autocomplete="off"; }
                br;
                label { "Password (optional): " input type="password" name="password" autocomplete="new-password"; }
                small { " — leave empty to have the server generate one" }
                br;
                datalist id="known-groups" {
                    @for group in groups {
                        option value=(group.name);
                    }
                }
                label { "Groups, both ways (optional): " input type="text" name="groups" list="known-groups" placeholder="Red, Blue"; }
                br;
                label { "Send-only groups (IN, optional): " input type="text" name="groups_in" list="known-groups"; }
                br;
                label { "Receive-only groups (OUT, optional): " input type="text" name="groups_out" list="known-groups"; }
                br;
                button type="submit" { "Create account" }
            }
        }
        @if users.is_empty() {
            p { "No password accounts." }
        } @else {
            table {
                tr { th{"Username"} th{"Groups"} th{"Created"} th{"Status"} th{} }
                @for user in users {
                    tr {
                        td { (user.username) }
                        td { (crate::client::describe_grants(&user.groups)) }
                        td { (user.created_at_unix) }
                        @if user.revoked {
                            td class="revoked" { "revoked" }
                        } @else {
                            td { "active" }
                        }
                        td {
                            @if !user.revoked {
                                form class="inline" method="post" action=(format!("/users/{}/revoke", urlencode(&user.username))) {
                                    button type="submit" class="danger" { "Revoke" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct MintUserForm {
    username: String,
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    password: Option<String>,
    #[serde(default)]
    groups: String,
    #[serde(default)]
    groups_in: String,
    #[serde(default)]
    groups_out: String,
}

async fn mint_user_page(State(state): State<AppState>, Form(form): Form<MintUserForm>) -> Response {
    let username = form.username.trim();
    if username.is_empty() {
        return error_page("A username is required.");
    }
    // Same as for tokens: check the QR code can be built before creating
    // anything, so a bad config doesn't leave an account half-handed-out.
    if let Some(url) = &state.enrollment_url
        && let Err(error) = EnrollmentLink::new(url, state.streaming_port, state.api_port, "x", "x")
    {
        return error_page(&format!("Can't build an enrollment QR code: {error}"));
    }
    let generated = form.password.is_none();
    match state
        .client
        .mint_user(
            username,
            form.password,
            crate::client::GroupLists {
                groups: name_list(&form.groups),
                groups_in: name_list(&form.groups_in),
                groups_out: name_list(&form.groups_out),
            },
        )
        .await
    {
        Ok(password) => {
            // ATAK's enrollment link presents `token` as the password, so a
            // password account gets the same QR code an invite token does.
            let link = state.enrollment_url.as_ref().and_then(|url| {
                EnrollmentLink::new(
                    url,
                    state.streaming_port,
                    state.api_port,
                    username,
                    &password,
                )
                .ok()
            });
            let body = html! {
                p { "Account " strong { (username) } " created." }
                @if generated {
                    p { "Generated password — this is the only time it is shown:" }
                    p { code { (password) } }
                }
                @if let Some(link) = &link {
                    p { "To enroll a TAK client (ATAK, OmniTAK, …) with this account, scan:" }
                    div { (link) }
                    p { "On the device itself, tap instead: " a href=(link.to_uri()) { "open in TAK client" } }
                    p { small { "The code contains the password and, unlike an invite token, keeps working until the account is revoked — show it only to its user." } }
                } @else {
                    p { "No QR code: set MICROTAK_ADMIN_WEB_ENROLLMENT_URL (where devices reach the server) to get one." }
                }
                p { a href="/users" { "Back to users" } }
            };
            Html(layout("Account Created", body).into_string()).into_response()
        }
        Err(error) => error_response(&error),
    }
}

async fn revoke_user_page(State(state): State<AppState>, Path(username): Path<String>) -> Response {
    match state.client.revoke_user(&username).await {
        Ok(()) => Redirect::to("/users").into_response(),
        Err(error) => error_response(&error),
    }
}

// ---------------------------------------------------------------------
// Devices: certificate revocation and identity blocks
// ---------------------------------------------------------------------

async fn list_devices_page(State(state): State<AppState>) -> Response {
    let certs = match state.client.list_certificates().await {
        Ok(certs) => certs,
        Err(error) => return error_response(&error),
    };
    // Connection status + last-seen comes from clientEndPoints (server 0.17.0+
    // keeps offline identities too). A convenience: if it can't be fetched we
    // still render the certificate list without the live column.
    let endpoints = state
        .client
        .list_client_endpoints()
        .await
        .unwrap_or_default();
    Html(layout("Devices", render_certificates(&certs, &endpoints)).into_string()).into_response()
}

/// The connection state of a device, matched from clientEndPoints by cert CN
/// or bound client uid.
enum Presence {
    Online,
    /// Offline, with the last-seen epoch seconds if the server reported one.
    Offline(Option<i64>),
    /// No clientEndPoints record at all (never connected, or old server).
    Unknown,
}

fn presence_of(
    cert: &crate::client::TakCert,
    endpoints: &[crate::client::ClientEndpoint],
) -> Presence {
    let matches = |e: &crate::client::ClientEndpoint| {
        e.common_name.as_deref() == Some(cert.user_dn.as_str())
            || (!cert.client_uid.is_empty() && e.uid.as_deref() == Some(cert.client_uid.as_str()))
    };
    if endpoints.iter().any(|e| matches(e) && e.is_connected()) {
        return Presence::Online;
    }
    match endpoints
        .iter()
        .filter(|e| matches(e))
        .map(|e| e.last_seen_unix)
        .max()
    {
        Some(ts) if ts > 0 => Presence::Offline(Some(ts)),
        Some(_) => Presence::Offline(None),
        None => Presence::Unknown,
    }
}

fn render_certificates(
    certs: &[crate::client::TakCert],
    endpoints: &[crate::client::ClientEndpoint],
) -> Markup {
    let now = now_unix();
    html! {
        p {
            "Every issued certificate (the official TAK Server certificate admin API). "
            "A device name shows " span class="text-emerald-600 dark:text-emerald-400 font-semibold" { "green" } " while it is connected. "
            strong { "Revoke" } " a lost or compromised device's certificate: it stops working immediately, live "
            "connection included; the owner can enroll a new device with their password account. "
            strong { "Block" } " (MicroTAK extension) locks an identity out entirely -- any certificate, no "
            "re-enrollment -- until unblocked."
        }
        @if certs.is_empty() {
            p { "No issued certificates." }
        } @else {
            table {
                tr { th{"ID"} th{"User"} th{"Status"} th{"Last seen"} th{"Client UID"} th{"Issued"} th{"Expires"} th{"Certificate"} th{} }
                @for cert in certs {
                    @let user = urlencode(&cert.user_dn);
                    @let presence = presence_of(cert, endpoints);
                    tr {
                        td { (cert.id) }
                        td {
                            @match presence {
                                Presence::Online => span class="text-emerald-600 dark:text-emerald-400 font-semibold" { (cert.user_dn) },
                                _ => span { (cert.user_dn) },
                            }
                        }
                        td class=(if cert.revocation_date.is_some() { "revoked" } else { "" }) {
                            (if cert.revocation_date.is_some() { "revoked" } else { "valid" })
                        }
                        td {
                            @match presence {
                                Presence::Online => span class="text-emerald-600 dark:text-emerald-400 font-semibold" { "online" },
                                Presence::Offline(Some(ts)) => span { (ago(now, ts)) },
                                Presence::Offline(None) => span { "offline" },
                                Presence::Unknown => span { "—" },
                            }
                        }
                        td { (if cert.client_uid.is_empty() { "—" } else { cert.client_uid.as_str() }) }
                        td { (cert.issuance_date.as_deref().unwrap_or("—")) }
                        td { (cert.expiration_date.as_deref().unwrap_or("—")) }
                        td { code { (short(&cert.hash)) } }
                        td {
                            @if cert.revocation_date.is_none() {
                                form class="inline" method="post" action=(format!("/devices/certificates/{}/revoke", urlencode(&cert.hash))) {
                                    button type="submit" class="danger" { "revoke" }
                                }
                                " "
                            }
                            form class="inline" method="post" action=(format!("/devices/{user}/block")) {
                                button type="submit" class="danger" { "block user" }
                            }
                            " "
                            form class="inline" method="post" action=(format!("/devices/{user}/unblock")) {
                                button type="submit" { "unblock" }
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn revoke_certificate_page(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Response {
    match state.client.revoke_certificate(&hash).await {
        Ok(()) => Redirect::to("/devices").into_response(),
        Err(error) => error_response(&error),
    }
}

async fn block_device_page(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    match state.client.set_device_blocked(&name, true).await {
        Ok(()) => Redirect::to("/devices").into_response(),
        Err(error) => error_response(&error),
    }
}

async fn unblock_device_page(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    match state.client.set_device_blocked(&name, false).await {
        Ok(()) => Redirect::to("/devices").into_response(),
        Err(error) => error_response(&error),
    }
}

// ---------------------------------------------------------------------
// Overview (default landing): the MicroTAK server's own live state
// ---------------------------------------------------------------------

async fn overview_page(State(state): State<AppState>) -> Response {
    // The server's CPU/RAM are shown as a share of the host, so gather the
    // host stats too; the operational figures come from the server's own
    // (private) metrics endpoint. A fetch failure is surfaced in the page,
    // not treated as fatal.
    let stats = crate::system::SystemStats::gather();
    let server = match &state.metrics_url {
        Some(url) => Some(crate::server_metrics::fetch(url).await),
        None => None,
    };
    Html(layout("Overview", render_overview(&stats, server.as_ref())).into_string()).into_response()
}

fn render_overview(
    stats: &crate::system::SystemStats,
    server: Option<&Result<crate::server_metrics::ServerMetrics, reqwest::Error>>,
) -> Markup {
    html! {
        // Keep the live figures current while someone is watching this page:
        // reload every 20s. The page is server-rendered, so a full reload is
        // the refresh (same "reload to refresh" the panel mentions).
        script { (maud::PreEscaped("setTimeout(function(){location.reload();}, 20000);")) }
        @match server {
            // Without the metrics endpoint there is nothing to show here --
            // point the operator at the knob that turns it on.
            None => {
                p class="my-4 rounded-xl border border-amber-300 bg-amber-50 p-4 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-300" {
                    "The server metrics endpoint is not configured — set "
                    code { "MICROTAK_ADMIN_WEB_METRICS_URL" }
                    " (or the module's "
                    code { "metricsUrl" }
                    ") to show the server's live state here."
                }
            }
            Some(result) => (render_server_panel(stats, result)),
        }
    }
}

/// The MicroTAK server's own live state: its operational figures (on top --
/// what the server is doing right now), then the server *process's* own
/// CPU/RAM as a share of the host (on the bottom), scraped from its metrics
/// endpoint.
fn render_server_panel(
    stats: &crate::system::SystemStats,
    result: &Result<crate::server_metrics::ServerMetrics, reqwest::Error>,
) -> Markup {
    use crate::system::human_bytes;
    let version = match result {
        Ok(m) => m.version.clone(),
        Err(_) => None,
    };
    html! {
        p class="text-sm text-slate-500 dark:text-slate-400" {
            "Live state of the MicroTAK server"
            @if let Some(v) = &version { " (v" (v) ")" }
            " — reload to refresh."
        }
        @match result {
            Err(_) => {
                p class="my-4 rounded-xl border border-amber-300 bg-amber-50 p-4 text-sm text-amber-800 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-300" {
                    "Server metrics endpoint is configured but unreachable right now."
                }
            }
            Ok(m) => {
                // Operational figures on top: what the server is doing now.
                (server_figures(m))
                // The server process's own resource use on the bottom.
                // CPU: cores busy now as a fraction of all cores, matching the
                // host card's 0-100 scale. Falls back to the cumulative counter.
                @let cpu_detail = match m.cpu_cores_used {
                    Some(cores) => format!("{cores:.2} cores busy · {:.0}s CPU time total", m.cpu_seconds),
                    None => format!("{:.0}s CPU time total (live sample unavailable)", m.cpu_seconds),
                };
                @let cpu_percent = match m.cpu_cores_used {
                    Some(cores) if stats.ncpu > 0 => (cores / stats.ncpu as f64 * 100.0).clamp(0.0, 100.0),
                    _ => 0.0,
                };
                (stat_card("Server CPU", &cpu_detail, cpu_percent))
                // RAM: the server's resident set as a share of host memory.
                @let host_bytes = stats.mem_total_kb.saturating_mul(1024);
                @let rss_percent = if host_bytes > 0 { (m.resident_bytes / host_bytes as f64 * 100.0).clamp(0.0, 100.0) } else { 0.0 };
                (stat_card(
                    "Server memory (RSS)",
                    &format!("{} of {} host RAM", human_bytes(m.resident_bytes as u64), human_bytes(host_bytes)),
                    rss_percent,
                ))
            }
        }
    }
}

// ---------------------------------------------------------------------
// System resource usage (this host)
// ---------------------------------------------------------------------

async fn system_page() -> Response {
    let stats = crate::system::SystemStats::gather();
    Html(layout("System", render_system(&stats)).into_string()).into_response()
}

fn render_system(stats: &crate::system::SystemStats) -> Markup {
    use crate::system::human_bytes;
    let cores = if stats.ncpu == 1 { "core" } else { "cores" };
    html! {
        p class="text-sm text-slate-500 dark:text-slate-400" { "Resource usage of the field-HQ host (this tablet) — reload to refresh." }
        (stat_card(
            "CPU load",
            &format!("{:.2} · {:.2} · {:.2}  (1·5·15 min, {} {cores})",
                stats.load1, stats.load5, stats.load15, stats.ncpu),
            stats.cpu_percent(),
        ))
        (stat_card(
            "Memory",
            &format!("{} / {} used",
                human_bytes(stats.mem_used_kb().saturating_mul(1024)),
                human_bytes(stats.mem_total_kb.saturating_mul(1024))),
            stats.mem_percent(),
        ))
        (stat_card(
            "Disk (/)",
            &format!("{} / {} used", human_bytes(stats.disk_used()), human_bytes(stats.disk_total)),
            stats.disk_percent(),
        ))
    }
}

/// A compact grid of the server's live operational figures (not usage bars).
fn server_figures(m: &crate::server_metrics::ServerMetrics) -> Markup {
    use crate::system::human_bytes;
    let figures: [(&str, String); 6] = [
        (
            "Connected clients",
            format!(
                "{} ({} TLS · {} TCP)",
                m.connected_total(),
                m.connected_tls,
                m.connected_tcp
            ),
        ),
        ("Open connections", m.open_connections.to_string()),
        ("Enrolled devices", m.devices.to_string()),
        (
            "Missions · groups",
            format!("{} · {}", m.missions, m.groups),
        ),
        ("CoT events relayed", m.cot_events_total().to_string()),
        ("Data stored", human_bytes(m.data_dir_bytes)),
    ];
    html! {
        div class="my-4 grid grid-cols-2 gap-3 sm:grid-cols-3" {
            @for (label, value) in &figures {
                div class="rounded-xl border border-slate-200 bg-white p-4 shadow-sm dark:border-slate-700 dark:bg-slate-800" {
                    div class="text-xs uppercase tracking-wide text-slate-400 dark:text-slate-500" { (label) }
                    div class="mt-1 text-lg font-semibold tabular-nums text-slate-800 dark:text-slate-100" { (value) }
                }
            }
        }
    }
}

/// A labelled usage card with a coloured progress bar (green < 70 %, amber
/// < 90 %, red above).
fn stat_card(title: &str, detail: &str, percent: f64) -> Markup {
    let bar = if percent >= 90.0 {
        "bg-rose-500"
    } else if percent >= 70.0 {
        "bg-amber-500"
    } else {
        "bg-emerald-500"
    };
    html! {
        div class="my-4 rounded-xl border border-slate-200 bg-white p-5 shadow-sm dark:border-slate-700 dark:bg-slate-800" {
            div class="flex items-baseline justify-between" {
                span class="font-semibold text-slate-800 dark:text-slate-100" { (title) }
                span class="text-sm tabular-nums text-slate-500 dark:text-slate-400" { (format!("{percent:.0}%")) }
            }
            div class="mt-1 text-sm text-slate-500 dark:text-slate-400" { (detail) }
            div class="mt-3 h-2.5 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-slate-700" {
                div class=(format!("h-full rounded-full {bar}")) style=(format!("width:{percent:.1}%")) {}
            }
        }
    }
}

// ---------------------------------------------------------------------
// Groups ("channels")
// ---------------------------------------------------------------------

async fn list_groups_page(State(state): State<AppState>) -> Response {
    match state.client.list_groups().await {
        Ok(groups) => Html(layout("Groups", render_groups(&groups)).into_string()).into_response(),
        Err(error) => error_response(&error),
    }
}

fn render_groups(groups: &[crate::client::GroupInfo]) -> Markup {
    html! {
        p {
            "As on the official TAK Server: a device " strong{"in"} " a group may send into it, a device "
            strong{"out"} " of a group receives from it. Devices without groups are in __ANON__ and see each "
            "other; missions are only visible within their groups."
        }
        fieldset {
            legend { "Create a group" }
            form method="post" action="/groups" {
                label { "Name: " input type="text" name="name" required; }
                br;
                label { "Description (optional): " input type="text" name="description"; }
                br;
                button type="submit" { "Create" }
            }
        }
        @for group in groups {
            h2 { (group.name) " " small { "(bitpos " (group.bitpos) ")" } }
            @if let Some(description) = &group.description { p { (description) } }
            table {
                tr { th{"Identity"} th{"Direction"} th{} }
                @for (identity, direction) in &group.members {
                    tr {
                        td { (identity) }
                        td { (direction.to_lowercase()) }
                        td {
                            form class="inline" method="post"
                                action=(format!("/groups/{}/members/{}/remove", urlencode(&group.name), urlencode(identity))) {
                                button type="submit" class="danger" { "Remove" }
                            }
                        }
                    }
                }
            }
            form method="post" action=(format!("/groups/{}/members", urlencode(&group.name))) {
                label { "Add / change member (CN): " input type="text" name="identity" required; }
                " "
                select name="direction" {
                    option value="BOTH" { "both" }
                    option value="IN" { "in (send only)" }
                    option value="OUT" { "out (receive only)" }
                }
                " "
                button type="submit" { "Set" }
            }
            @if group.name != "__ANON__" {
                form class="inline" method="post" action=(format!("/groups/{}/delete", urlencode(&group.name))) {
                    button type="submit" class="danger" { "Delete group " (group.name) }
                }
            }
        }
    }
}

#[derive(Deserialize)]
struct CreateGroupForm {
    name: String,
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    description: Option<String>,
}

async fn create_group_page(
    State(state): State<AppState>,
    Form(form): Form<CreateGroupForm>,
) -> Response {
    match state
        .client
        .create_group(form.name.trim(), form.description)
        .await
    {
        Ok(()) => Redirect::to("/groups").into_response(),
        Err(error) => error_response(&error),
    }
}

async fn delete_group_page(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    match state.client.delete_group(&name).await {
        Ok(()) => Redirect::to("/groups").into_response(),
        Err(error) => error_response(&error),
    }
}

#[derive(Deserialize)]
struct SetMemberForm {
    identity: String,
    direction: String,
}

async fn set_member_page(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Form(form): Form<SetMemberForm>,
) -> Response {
    match state
        .client
        .set_group_member(&name, form.identity.trim(), &form.direction)
        .await
    {
        Ok(()) => Redirect::to("/groups").into_response(),
        Err(error) => error_response(&error),
    }
}

async fn remove_member_page(
    State(state): State<AppState>,
    Path((name, identity)): Path<(String, String)>,
) -> Response {
    match state.client.remove_group_member(&name, &identity).await {
        Ok(()) => Redirect::to("/groups").into_response(),
        Err(error) => error_response(&error),
    }
}

// ---------------------------------------------------------------------
// Missions / roles
// ---------------------------------------------------------------------

async fn list_missions_page(State(state): State<AppState>) -> Response {
    let missions = match state.client.list_missions().await {
        Ok(missions) => missions,
        Err(error) => return error_response(&error),
    };
    // Group picker is a convenience; render the form even if groups can't be
    // fetched (the field accepts typed names too).
    let groups = state.client.list_groups().await.unwrap_or_default();
    let body = html! {
        fieldset {
            legend { "Create a mission" }
            form method="post" action="/missions" {
                label { "Name: " input type="text" name="name" placeholder="e.g. recon-east" required; }
                br;
                label { "Description (optional): " input type="text" name="description"; }
                br;
                // A mission is visible only within its groups; share the
                // server's existing groups as a dropdown (comma-separated;
                // empty means __ANON__, visible to everyone).
                datalist id="known-groups" {
                    @for group in &groups {
                        option value=(group.name);
                    }
                }
                label { "Groups (optional, comma-separated): " input type="text" name="groups" list="known-groups" placeholder="Red, Blue — empty = __ANON__"; }
                br;
                label { "Creator UID: " input type="text" name="creator_uid" value="admin"; }
                br;
                label {
                    "Default role for new subscribers: "
                    select name="default_role" {
                        option value="MISSION_SUBSCRIBER" { "subscriber" }
                        option value="MISSION_OWNER" { "owner" }
                        option value="MISSION_READONLY_SUBSCRIBER" { "read-only subscriber" }
                    }
                }
                small { " — needed to manage roles later" }
                br;
                button type="submit" { "Create mission" }
            }
        }
        @if missions.is_empty() {
            p { "No missions yet." }
        } @else {
            table {
                tr { th{"Name"} th{"Creator"} th{"Groups"} th{"Default role"} th{} }
                @for mission in &missions {
                    tr {
                        td { (mission.name) }
                        td { (mission.creator_uid) }
                        td { (mission.groups.join(", ")) }
                        td { (mission.default_role.as_ref().map(|r| r.role.as_str()).unwrap_or("-")) }
                        td { a href=(format!("/missions/{}", urlencode(&mission.name))) { "Manage" } }
                    }
                }
            }
        }
    };
    Html(layout("Missions", body).into_string()).into_response()
}

#[derive(Deserialize)]
struct CreateMissionForm {
    name: String,
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    description: Option<String>,
    #[serde(default)]
    groups: String,
    #[serde(default)]
    creator_uid: String,
    #[serde(default)]
    default_role: String,
}

async fn create_mission_page(
    State(state): State<AppState>,
    Form(form): Form<CreateMissionForm>,
) -> Response {
    let name = form.name.trim();
    if name.is_empty() {
        return error_page("A mission needs a name.");
    }
    let creator_uid = match form.creator_uid.trim() {
        "" => "admin",
        other => other,
    };
    let default_role = match form.default_role.trim() {
        "" => None,
        role => Some(role),
    };
    match state
        .client
        .create_mission(
            name,
            creator_uid,
            form.description.as_deref(),
            &name_list(&form.groups),
            default_role,
        )
        .await
    {
        Ok(()) => Redirect::to(&format!("/missions/{}", urlencode(name))).into_response(),
        Err(error) => error_response(&error),
    }
}

fn urlencode(segment: &str) -> String {
    // Path segments here only ever come from names microtak-server itself
    // already returned to us, but they can still contain spaces/'&' (see
    // TC-MARTI-03) -- re-encode for use in an href.
    let mut out = String::new();
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

async fn mission_detail_page(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    match state.client.get_mission(&name).await {
        Ok(Some((mission, subscriptions))) => {
            let roles_enabled = mission.default_role.is_some();
            let body = html! {
                p { "Creator: " (mission.creator_uid) }
                p { "Visible in groups: " (mission.groups.join(", ")) }
                p { "New subscribers get: " (mission.default_role.as_ref().map(|r| r.role.as_str()).unwrap_or("MISSION_SUBSCRIBER (default role not set)")) }
                table {
                    tr { th{"Client uid"} th{"User"} th{"Role"} th{} }
                    @for sub in &subscriptions {
                        tr {
                            td { (sub.client_uid) }
                            td { (sub.username) }
                            td { (sub.role.role) }
                            td {
                                @if roles_enabled {
                                    form class="inline" method="post" action=(format!("/missions/{}/roles/{}/revoke", urlencode(&name), urlencode(&sub.client_uid))) {
                                        button type="submit" class="danger" { "Remove" }
                                    }
                                }
                            }
                        }
                    }
                }
                @if roles_enabled {
                    fieldset {
                        legend { "Set a role" }
                        form method="post" action=(format!("/missions/{}", urlencode(&name))) {
                            label { "Client uid: " input type="text" name="uid" required; }
                            br;
                            label {
                                "Role: "
                                select name="role" {
                                    option value="MISSION_OWNER" { "owner" }
                                    option value="MISSION_SUBSCRIBER" { "subscriber" }
                                    option value="MISSION_READONLY_SUBSCRIBER" { "read-only subscriber" }
                                }
                            }
                            br;
                            button type="submit" { "Set" }
                        }
                    }
                } @else {
                    p { "Roles can only be changed on missions with a default role (as on the official TAK Server)." }
                }
                p { a href="/missions" { "Back to mission list" } }
            };
            Html(layout(&format!("Mission: {name}"), body).into_string()).into_response()
        }
        Ok(None) => {
            Html(layout("Not Found", error_banner("no such mission")).into_string()).into_response()
        }
        Err(error) => error_response(&error),
    }
}

#[derive(Deserialize)]
struct AssignRoleForm {
    uid: String,
    role: String,
}

async fn assign_role_page(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Form(form): Form<AssignRoleForm>,
) -> Response {
    match state
        .client
        .set_role(&name, &form.uid, Some(&form.role))
        .await
    {
        Ok(()) => Redirect::to(&format!("/missions/{}", urlencode(&name))).into_response(),
        Err(error) => error_response(&error),
    }
}

async fn revoke_role_page(
    State(state): State<AppState>,
    Path((name, uid)): Path<(String, String)>,
) -> Response {
    match state.client.set_role(&name, &uid, None).await {
        Ok(()) => Redirect::to(&format!("/missions/{}", urlencode(&name))).into_response(),
        Err(error) => error_response(&error),
    }
}
