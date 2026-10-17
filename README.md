# microtak-admin-web

A small, standalone web UI for managing a [microtak-server](https://github.com/microtak/microtak-server)
instance: enrollment invite tokens (with QR codes), groups, mission roles
and devices (certificate revocation, identity blocks). It's a thin client over microtak-server's existing mTLS admin
API — it has no compile-time dependency on microtak-server at runtime, and
holds no state of its own.

MicroTAK is not affiliated with or endorsed by the U.S. Department of Defense, the TAK Product Center, or any other organization.

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
| `MICROTAK_ADMIN_WEB_ENROLLMENT_URL` | no | Where devices reach microtak-server's enrollment endpoint (HTTPS only), e.g. `https://192.168.1.10:8446`, or `https://tak.example.com` behind a reverse proxy on 443 — needed for enrollment QR codes |
| `MICROTAK_ADMIN_WEB_STREAMING_PORT` | no | Streaming (mTLS CoT) port devices connect to, default `8089` — added to the QR code only if different |
| `MICROTAK_ADMIN_WEB_API_PORT` | no | Marti API port devices connect to, default `8443` — added to the QR code only if different |

### Bootstrapping the admin certificate

This tool doesn't enroll itself. Enroll the admin device first with the
server's one-time bootstrap token (microtak-server writes it to
`data_dir/bootstrap-token` on first start; see its README, or
`microtak-admin-cli enroll`), then point `MICROTAK_ADMIN_WEB_CERT`/`_KEY`/
`_CA` at the issued credentials.

Version 0.4 and later use the official Mission API and need
microtak-server 0.9 or later.

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
  unused/used/expired/revoked, device it's bound to, note, expiry), mint a
  new one (device name, optional expiry and note), and get the standard TAK
  enrollment QR code for it — an inline SVG generated server-side, plus a
  tap-to-open `tak://` link for when the page is open on the device itself.
- **`/groups`** — groups ("channels", as on the official TAK Server):
  create and delete groups, add members with a direction (**in** = may send
  into the group, **out** = receives from it, or both), remove members.
  Tokens can carry groups too (both ways / send-only / receive-only), so a
  device scanning its QR code lands in its team. Devices without groups are
  in `__ANON__`; missions are only visible within their groups.
- **`/missions`** and **`/missions/:name`** — list missions with the groups
  they're visible in, and per mission, see and change its subscribers'
  roles (`MISSION_OWNER`, `MISSION_SUBSCRIBER`,
  `MISSION_READONLY_SUBSCRIBER`) through the official Mission API. Errors
  from the server (e.g. a caller who isn't the mission's owner) show up as
  a readable message in the page, not a stack trace.
- **`/devices`** — every issued certificate from the official TAK Server
  certificate admin API (id, user, status, client uid, dates, hash).
  "revoke" for a lost device's certificate (it stops working at once, live
  connection included; the owner re-enrolls with their account),
  "block user"/"unblock" to lock an identity out entirely (MicroTAK
  extension).

## QR code payload

The standard ATAK enrollment link — the same one `microtak-admin-cli token
mint --qr` prints:

```
tak://com.atakmap.app/enroll?host=<host>&username=<device name>&token=<token>
```

Scanning it in ATAK or OmniTAK enrolls the device over HTTPS (presenting
the token as the password for that device name) and connects it — nothing
to type. `host` and the enrollment port come from
`MICROTAK_ADMIN_WEB_ENROLLMENT_URL`; OmniTAK's optional `enrollmentport=`,
`port=` and `apiport=` are added only when they differ from 8446/8089/8443.
A QR code is only shown when a device name was entered: the token is bound
to that identity server-side and can't enroll any other. Anyone who sees
the code before the device uses it can enroll as that device, so show it
only to its user.

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

Since browsers send Basic Auth credentials with any request to this
origin, every state-changing request must come from this UI itself
(`Sec-Fetch-Site: same-origin`, or an `Origin`/`Referer` matching the
host) — a form that another site makes your browser submit is refused.
Scripts should use microtak-admin-cli instead.

## Security

Run it on localhost or a trusted network only (the default bind is
`127.0.0.1:8090`); see "Auth" above for why. It holds the admin
certificate and key, so keep those owner-only. To report a vulnerability,
see [SECURITY.md](SECURITY.md).

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

`scripts/build.sh` runs the same three steps and checks the sibling
checkout is present first, with a clear error if it isn't; see
`scripts/build.sh --help`.

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
