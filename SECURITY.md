# Security policy

microtak-admin-web holds a MicroTAK server's admin certificate and key,
which can enroll, revoke and reassign every device and mission on that
server. Security reports are very welcome.

## Supported versions

Fixes go into the latest release only, published as a patch release.
Please upgrade to the latest release before reporting.

## Reporting a vulnerability

**Please don't open a public issue.** Report privately through GitHub:
[Security → Report a vulnerability](https://github.com/microtak/microtak-admin-web/security/advisories/new).

Helpful to include the version, how you run it, what an attacker needs,
and steps or a proof of concept. Reports are handled as soon as possible;
this is a small project without a formal response-time commitment. You'll
get credit in the advisory unless you prefer not to.

Vulnerabilities in microtak-server itself go to
[microtak-server](https://github.com/microtak/microtak-server/security/advisories/new).

## Scope and hardening

In scope: the web UI, its auth gate and its handling of the admin
credentials. Exposing it to an untrusted network is out of scope; the
README says to keep it on localhost or behind your own access control.

- Every page needs HTTP Basic Auth with `MICROTAK_ADMIN_WEB_PASSWORD`;
  it listens on `127.0.0.1` by default.
- State-changing requests must come from the UI itself
  (`Sec-Fetch-Site`, `Origin`/`Referer`), so other sites can't submit
  forms through your browser.
- The server is verified against `MICROTAK_ADMIN_WEB_CA`; the tool keeps
  no state of its own.
- Dependencies are scanned with OSV-Scanner in CI.
