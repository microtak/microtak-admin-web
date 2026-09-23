# microtak-admin-web

A small, standalone web UI for managing a [microtak-server](https://github.com/microtak/microtak-server)
instance: enrollment invite tokens (with QR codes) and mission role
assignment. It's a thin client over microtak-server's existing mTLS admin
API — it has no compile-time dependency on microtak-server at runtime, and
holds no state of its own.

## Why a separate tool, not a feature of microtak-server itself

microtak-server's own design deliberately keeps its core lightweight — no
web UI framework, no session/cookie auth, nothing beyond the Marti API
surface a real TAK client needs. This tool is the "separate, optional
connector" pattern that project already uses for other things (radio
bridges, `microtak-sync`, `microtak-admin-cli`): it talks to
microtak-server only over its existing HTTP API, using a real, already-
issued admin mTLS certificate — the same way any other API client would,
not by being linked into the server's own binary.

## Configuration

All via environment variables:

| Variable | Required | Meaning |
|---|---|---|
| `MICROTAK_ADMIN_WEB_SERVER` | yes | microtak-server's Marti API base URL, e.g. `https://microtak.example.com:8443` |
| `MICROTAK_ADMIN_WEB_CERT` | yes | Path to the admin device's client certificate PEM |
| `MICROTAK_ADMIN_WEB_KEY` | yes | Path to that certificate's private key PEM |
| `MICROTAK_ADMIN_WEB_CA` | yes | Path to microtak-server's CA certificate PEM |
| `MICROTAK_ADMIN_WEB_PASSWORD` | yes | Shared secret required to use this web UI at all (HTTP Basic Auth) — see "Auth" below |
| `MICROTAK_ADMIN_WEB_BIND` | no (default `127.0.0.1:8090`) | Where this tool itself listens |
| `MICROTAK_ADMIN_WEB_ENROLLMENT_URL` | no | microtak-server's *enrollment* endpoint base URL (different port, plain HTTP, e.g. `http://microtak.example.com:8446`) — included in minted tokens' QR codes if set |

### Bootstrapping the admin certificate

This tool doesn't enroll itself. You need an admin device already enrolled
against microtak-server first — see that project's
`docs/ARCHITECTURE.md` "Enrollment lockdown / admin API" section for the
real two-phase bootstrap flow (enroll the admin device while enrollment is
still open, *then* turn `enrollment_requires_token` on). Point
`MICROTAK_ADMIN_WEB_CERT`/`_KEY`/`_CA` at that admin device's issued
credentials.

## Running

```sh
cargo build --release
MICROTAK_ADMIN_WEB_SERVER=https://microtak.example.com:8443 \
MICROTAK_ADMIN_WEB_CERT=./admin.pem \
MICROTAK_ADMIN_WEB_KEY=./admin.key \
MICROTAK_ADMIN_WEB_CA=./ca.pem \
MICROTAK_ADMIN_WEB_PASSWORD=change-me \
  ./target/release/microtak-admin-web
```

Then open `http://127.0.0.1:8090/` (or whatever `MICROTAK_ADMIN_WEB_BIND`
you set) in a browser — you'll be prompted for HTTP Basic Auth; any
username, the password must match `MICROTAK_ADMIN_WEB_PASSWORD`.

## What it does

- **`/tokens`** — list existing enrollment invite tokens (status:
  unused/used/expired/revoked, note, expiry), mint a new one (optional
  expiry, optional note), and see the newly-minted token rendered as a
  real inline SVG QR code — generated server-side, not via a client-side
  JS library.
- **`/missions`** and **`/missions/:name`** — list missions, and per
  mission, see and manage its `Owner`/`Subscriber` role assignments.
  Assigning/revoking a role that would leave a mission with zero owners
  (microtak-server's own last-owner protection) surfaces as a real,
  readable error in the page, not a stack trace.

## QR code payload

**This is MicroTAK's own scheme, not a claimed-compatible ATAK/Marti
standard** — no authoritative source for a real one was found. The QR
encodes a JSON object:

```json
{"microtakEnroll": {"token": "<the token>", "enrollmentUrl": "<optional, if MICROTAK_ADMIN_WEB_ENROLLMENT_URL is set>"}}
```

Deliberately plain JSON (not a bespoke binary encoding) so it's
inspectable by hand and trivial for a future `microtak-admin-cli` or
`microtak-node` consumer to parse.

## Auth — a deliberate v1 simplification

`MICROTAK_ADMIN_WEB_PASSWORD` gates every page with HTTP Basic Auth. This
is **not** a real user-account system: there's no per-operator identity,
no audit trail of *who* took a given action through this UI (only that the
one shared admin mTLS identity did — same as raw `curl` would show), and
no password rotation/expiry. This tool holds a powerful admin credential
and can take destructive actions (revoke tokens, strip a mission's owner
role), so *some* gate is required before anyone can reach it — this is
that gate, not a finished access-control system. Run it behind your own
network-level access control (VPN, a reverse proxy with real auth) in
addition to, not instead of, this check for anything beyond a trusted LAN.

## Development

```sh
cargo build --tests
cargo test
cargo clippy --all-targets -- -D warnings
```

`tests/e2e.rs` starts a real `microtak_server::app::App` (a `[dev-
dependencies]`-only path dependency on the sibling `../microtak-server`
checkout — this tool's actual runtime binary has no dependency on that
crate at all) and drives this tool's real router against it end to end.

## License

AGPL-3.0-or-later — see [LICENSE](LICENSE). Same reasoning as
microtak-server itself: this tool holds a powerful admin credential, and
the license is chosen specifically so a modified version run as a hosted
service has to share its changes back.

## Project layout

```
src/
  lib.rs       — module map
  main.rs      — thin binary entry point (reads config, wires everything, serves)
  config.rs    — environment-variable configuration
  client.rs    — HTTP client for microtak-server's admin/mission-role API
  pages.rs     — all HTML routes/handlers (maud templates, plain HTML forms)
  auth.rs      — HTTP Basic Auth gate (see "Auth" above)
  qr.rs        — enrollment-token QR code rendering
tests/
  e2e.rs       — end-to-end suite against a real microtak-server App
```
