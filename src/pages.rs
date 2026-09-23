//! All HTML pages: server-rendered with `maud`, forms submit as plain
//! `application/x-www-form-urlencoded` POSTs -- no client-side JS
//! framework, no build step, matching this project's lightweight ethos.

use std::sync::Arc;

use axum::extract::{Form, Path, State};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use maud::{html, Markup, DOCTYPE};
use serde::Deserialize;

use crate::client::{ClientError, MicrotakClient};
use crate::qr::EnrollmentQr;

#[derive(Clone)]
pub struct AppState {
    pub client: Arc<MicrotakClient>,
    pub enrollment_url: Option<String>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(|| async { Redirect::to("/tokens") }))
        .route("/tokens", get(list_tokens_page).post(mint_token_page))
        .route("/tokens/:token/revoke", post(revoke_token_page))
        .route("/missions", get(list_missions_page))
        .route("/missions/:name", get(mission_detail_page).post(assign_role_page))
        .route(
            "/missions/:name/roles/:uid/revoke",
            post(revoke_role_page),
        )
        .with_state(state)
}

fn layout(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                title { "microtak-admin-web — " (title) }
                style { (maud::PreEscaped(CSS)) }
            }
            body {
                nav {
                    a href="/tokens" { "Enrollment Tokens" }
                    " · "
                    a href="/missions" { "Missions" }
                }
                hr;
                h1 { (title) }
                (body)
            }
        }
    }
}

// Kept tiny and inline rather than a separate static asset, consistent
// with "no build step."
const CSS: &str = "body{font-family:sans-serif;max-width:860px;margin:2rem auto;padding:0 1rem;color:#111}\
    table{border-collapse:collapse;width:100%;margin:1rem 0}\
    th,td{border:1px solid #ccc;padding:0.4rem 0.6rem;text-align:left;font-size:0.9rem}\
    .error{background:#fee;border:1px solid #c33;padding:0.6rem;margin:1rem 0}\
    .revoked,.used{color:#888}\
    form.inline{display:inline}\
    fieldset{margin:1.5rem 0}\
    code{background:#f2f2f2;padding:0.1rem 0.3rem}";

fn error_banner(message: &str) -> Markup {
    html! { div class="error" { (message) } }
}

fn error_response(error: &ClientError) -> Response {
    Html(layout("Error", error_banner(&error.to_string())).into_string()).into_response()
}

// ---------------------------------------------------------------------
// Enrollment tokens
// ---------------------------------------------------------------------

async fn list_tokens_page(State(state): State<AppState>) -> Response {
    match state.client.list_tokens().await {
        Ok(tokens) => Html(layout("Enrollment Tokens", render_tokens_list(&tokens)).into_string())
            .into_response(),
        Err(error) => error_response(&error),
    }
}

fn render_tokens_list(tokens: &[crate::client::EnrollmentToken]) -> Markup {
    html! {
        fieldset {
            legend { "Mint a new token" }
            form method="post" action="/tokens" {
                label { "Expires in (seconds, optional): " input type="number" name="expires_in_secs"; }
                br;
                label { "Note (optional): " input type="text" name="note"; }
                br;
                button type="submit" { "Mint token" }
            }
        }
        table {
            tr { th{"Token"} th{"Note"} th{"Created"} th{"Expires"} th{"Status"} th{} }
            @for token in tokens {
                tr {
                    td { code { (short(&token.token)) } }
                    td { (token.note.as_deref().unwrap_or("—")) }
                    td { (token.created_at_unix) }
                    td { (token.expires_at_unix.map(|t| t.to_string()).unwrap_or_else(|| "never".to_string())) }
                    td class=(status_class(token)) { (status_text(token)) }
                    td {
                        @if !token.used && !token.revoked {
                            form class="inline" method="post" action=(format!("/tokens/{}/revoke", token.token)) {
                                button type="submit" { "Revoke" }
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
        format!("used by {}", token.used_by_common_name.as_deref().unwrap_or("?"))
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

#[derive(Deserialize)]
struct MintTokenForm {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    expires_in_secs: Option<i64>,
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    note: Option<String>,
}

fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    match raw.as_deref() {
        None | Some("") => Ok(None),
        Some(value) => value
            .parse()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn empty_string_as_none_str<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: Option<String> = Option::deserialize(deserializer)?;
    Ok(raw.filter(|s| !s.is_empty()))
}

async fn mint_token_page(State(state): State<AppState>, Form(form): Form<MintTokenForm>) -> Response {
    match state.client.mint_token(form.expires_in_secs, form.note).await {
        Ok(token) => {
            let qr = EnrollmentQr {
                token: token.clone(),
                enrollment_url: state.enrollment_url.clone(),
            };
            let body = html! {
                p { "Token minted. This is the only time the full value is shown here:" }
                p { code { (token) } }
                div { (qr) }
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
// Missions / roles
// ---------------------------------------------------------------------

async fn list_missions_page(State(state): State<AppState>) -> Response {
    match state.client.list_missions().await {
        Ok(missions) => {
            let body = html! {
                table {
                    tr { th{"Name"} th{"Creator"} th{"Roles"} th{} }
                    @for mission in &missions {
                        tr {
                            td { (mission.name) }
                            td { (mission.creator_uid) }
                            td { (mission.roles.len()) }
                            td { a href=(format!("/missions/{}", urlencode(&mission.name))) { "Manage" } }
                        }
                    }
                }
            };
            Html(layout("Missions", body).into_string()).into_response()
        }
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
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

async fn mission_detail_page(State(state): State<AppState>, Path(name): Path<String>) -> Response {
    match state.client.get_mission(&name).await {
        Ok(Some(mission)) => {
            let body = html! {
                p { "Creator: " (mission.creator_uid) }
                table {
                    tr { th{"Identity"} th{"Role"} th{} }
                    @for (uid, role) in &mission.roles {
                        tr {
                            td { (uid) }
                            td { (role) }
                            td {
                                form class="inline" method="post" action=(format!("/missions/{}/roles/{}/revoke", urlencode(&name), urlencode(uid))) {
                                    button type="submit" { "Revoke" }
                                }
                            }
                        }
                    }
                }
                fieldset {
                    legend { "Assign a role" }
                    form method="post" action=(format!("/missions/{}", urlencode(&name))) {
                        label { "Identity (CN): " input type="text" name="uid" required; }
                        br;
                        label {
                            "Role: "
                            select name="role" {
                                option value="owner" { "owner" }
                                option value="subscriber" { "subscriber" }
                            }
                        }
                        br;
                        button type="submit" { "Assign" }
                    }
                }
                p { a href="/missions" { "Back to mission list" } }
            };
            Html(layout(&format!("Mission: {name}"), body).into_string()).into_response()
        }
        Ok(None) => Html(layout("Not Found", error_banner("no such mission")).into_string()).into_response(),
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
    match state.client.assign_role(&name, &form.uid, &form.role).await {
        Ok(()) => Redirect::to(&format!("/missions/{}", urlencode(&name))).into_response(),
        Err(error) => error_response(&error),
    }
}

async fn revoke_role_page(
    State(state): State<AppState>,
    Path((name, uid)): Path<(String, String)>,
) -> Response {
    match state.client.revoke_role(&name, &uid).await {
        Ok(()) => Redirect::to(&format!("/missions/{}", urlencode(&name))).into_response(),
        // The last-owner-protection 409 (or any other rejection) surfaces
        // as a real, readable error here, not a panic or a raw stack trace.
        Err(error) => error_response(&error),
    }
}
