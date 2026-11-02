{
  description = "microtak-admin-web: standalone web admin UI for a MicroTAK server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    { self, nixpkgs, flake-utils, rust-overlay, crane }:
    flake-utils.lib.eachSystem
      [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ]
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          rustToolchain = pkgs.rust-bin.stable.latest.default;
          craneLib = (crane.mkLib pkgs).overrideToolchain (_: rustToolchain);

          # cleanCargoSource keeps only Rust/Cargo files; the compiled stylesheet
          # (assets/app.css) is include_str!'d into the binary, so keep assets/ too.
          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            name = "source";
            filter =
              path: type:
              (craneLib.filterCargoSources path type) || (builtins.match ".*/assets(/.*)?$" path != null);
          };

          commonArgs = {
            inherit src;
            strictDeps = true;
            buildInputs = pkgs.lib.optionals pkgs.stdenv.hostPlatform.isDarwin [
              pkgs.libiconv
              pkgs.darwin.apple_sdk.frameworks.Security
              pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
            ];
            # Cargo.toml has a [dev-dependencies] path dependency on a sibling
            # ../microtak-server checkout (only tests/e2e.rs uses it). Even a
            # plain `cargo build` resolves the whole manifest, so the path has
            # to exist, at the version Cargo.lock records -- a stub crate does
            # it without pulling the real server into the build. The stub has
            # none of the real crate's dependencies, so cargo drops those from
            # the lock: build --offline rather than --locked (every crate is
            # still the one Cargo.lock pins, vendored from it; nothing is fetched).
            cargoExtraArgs = "--offline";
            postUnpack = ''
              mkdir -p "$NIX_BUILD_TOP/microtak-server/src"
              cat > "$NIX_BUILD_TOP/microtak-server/Cargo.toml" <<'EOF'
              [package]
              name = "microtak-server"
              version = "0.9.0"
              edition = "2021"
              [lib]
              path = "src/lib.rs"
              EOF
              : > "$NIX_BUILD_TOP/microtak-server/src/lib.rs"
            '';
          };

          cargoArtifacts = craneLib.buildDepsOnly commonArgs;

          microtak-admin-web = craneLib.buildPackage (
            commonArgs
            // {
              inherit cargoArtifacts;
              pname = "microtak-admin-web";
              cargoExtraArgs = "--offline --bin microtak-admin-web";
              # The only test is the e2e one, which needs the real server
              # crate; CI runs it against a microtak-server checkout.
              doCheck = false;
              meta.mainProgram = "microtak-admin-web";
            }
          );
        in
        {
          packages = {
            default = microtak-admin-web;
            microtak-admin-web = microtak-admin-web;
          };

          apps.default = flake-utils.lib.mkApp {
            drv = microtak-admin-web;
            name = "microtak-admin-web";
          };

          checks = {
            inherit microtak-admin-web;

            # Lib and bin only: --all-targets would compile tests/e2e.rs,
            # which needs the real server crate (see postUnpack above).
            clippy = craneLib.cargoClippy (
              commonArgs
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--lib --bins -- -D warnings";
              }
            );
          };

          devShells.default = pkgs.mkShell {
            inputsFrom = [ microtak-admin-web ];
            packages = [ rustToolchain ];
          };
        }
      )
    // {
      # The UI as a system service: a thin mTLS client over the server's Marti
      # admin API (enrollment invite tokens with QR codes, groups, mission
      # roles, device revocation); it holds no state of its own.
      #
      # It authenticates to the server with the admin device's mTLS credential,
      # gated by a group (default `microtak`) -- so the service joins that group
      # rather than owning its own copy of the cert. Behind that, the UI itself
      # is protected by HTTP Basic auth; the password is generated on first
      # start into an EnvironmentFile under the credentials dir.
      nixosModules.default =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.microtak-admin-web;
        in
        {
          options.services.microtak-admin-web = {
            enable = lib.mkEnableOption "the MicroTAK web admin UI";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.microtak-admin-web;
              description = "The microtak-admin-web package to run.";
            };

            serverUrl = lib.mkOption {
              type = lib.types.str;
              example = "https://tak.example.org:8443";
              description = "MicroTAK server's Marti API base URL (a host name the server cert carries).";
            };

            enrollmentUrl = lib.mkOption {
              type = lib.types.str;
              example = "https://192.168.1.10:8446";
              description = "Where devices reach the server's enrollment endpoint (for the QR codes).";
            };

            bindAddress = lib.mkOption {
              type = lib.types.str;
              default = "127.0.0.1:8090";
              description = "Where this UI listens. Use 0.0.0.0:8090 for LAN reach.";
            };

            metricsUrl = lib.mkOption {
              type = lib.types.nullOr lib.types.str;
              default = null;
              example = "http://127.0.0.1:9090";
              description = ''
                Base URL of the microtak-server's Prometheus metrics endpoint
                (plain HTTP, kept private). Set it to show the server process's
                own CPU/RAM and live counters on the System page. Requires
                `metrics_port` in the server's config (and that port kept off
                the firewall).
              '';
            };

            credentialsDir = lib.mkOption {
              type = lib.types.path;
              default = "/var/lib/microtak-admin";
              description = ''
                Directory holding the admin mTLS credential
                (admin.pem/admin.key/ca.pem), readable by `group`. The
                first-start password EnvironmentFile (web.env) and a
                human-readable copy (web-password) are written here too.
              '';
            };

            group = lib.mkOption {
              type = lib.types.str;
              default = "microtak";
              description = "Group that can read the admin credential; the service joins it.";
            };
          };

          config = lib.mkIf cfg.enable {
            # First start: mint the Basic-auth password into an EnvironmentFile
            # (and a readable copy), unless one already exists. Runs as root so
            # it can write into the dir.
            systemd.services.microtak-admin-web-setup = {
              description = "microtak-admin-web first-start setup (Basic-auth password)";
              wantedBy = [ "multi-user.target" ];
              before = [ "microtak-admin-web.service" ];
              serviceConfig = {
                Type = "oneshot";
                RemainAfterExit = true;
              };
              path = [
                pkgs.openssl
                pkgs.coreutils
              ];
              script = ''
                envf="${cfg.credentialsDir}/web.env"
                if [ ! -e "$envf" ]; then
                  pw="$(openssl rand -base64 18 | tr -d '/+=' | cut -c1-24)"
                  umask 027
                  printf 'MICROTAK_ADMIN_WEB_PASSWORD=%s\n' "$pw" > "$envf"
                  printf '%s\n' "$pw" > "${cfg.credentialsDir}/web-password"
                  chgrp ${cfg.group} "$envf" "${cfg.credentialsDir}/web-password" || true
                  chmod 0640 "$envf" "${cfg.credentialsDir}/web-password"
                  echo "microtak-admin-web: generated Basic-auth password at ${cfg.credentialsDir}/web-password"
                fi
              '';
            };

            systemd.services.microtak-admin-web = {
              description = "MicroTAK web admin UI";
              wantedBy = [ "multi-user.target" ];
              after = [
                "network.target"
                "microtak-server.service"
                "microtak-admin-web-setup.service"
              ];
              requires = [ "microtak-admin-web-setup.service" ];
              environment = {
                MICROTAK_ADMIN_WEB_SERVER = cfg.serverUrl;
                MICROTAK_ADMIN_WEB_ENROLLMENT_URL = cfg.enrollmentUrl;
                MICROTAK_ADMIN_WEB_BIND = cfg.bindAddress;
                MICROTAK_ADMIN_WEB_CERT = "${cfg.credentialsDir}/admin.pem";
                MICROTAK_ADMIN_WEB_KEY = "${cfg.credentialsDir}/admin.key";
                MICROTAK_ADMIN_WEB_CA = "${cfg.credentialsDir}/ca.pem";
              }
              // lib.optionalAttrs (cfg.metricsUrl != null) {
                MICROTAK_ADMIN_WEB_METRICS_URL = cfg.metricsUrl;
              };
              serviceConfig = {
                ExecStart = lib.getExe cfg.package;
                EnvironmentFile = "${cfg.credentialsDir}/web.env";
                Restart = "on-failure";
                RestartSec = 3;
                DynamicUser = true;
                SupplementaryGroups = [ cfg.group ];
                # Read-only system; it only reads the credential dir and
                # listens on a socket.
                ProtectSystem = "strict";
                ProtectHome = true;
                NoNewPrivileges = true;
                PrivateTmp = true;
              };
            };
          };
        };
    };
}
