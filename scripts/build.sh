#!/usr/bin/env bash
# Local build + test, mirroring .github/workflows/ci.yml.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

usage() {
  cat <<'EOF'
Usage: scripts/build.sh [options]

  -b, --build     Build only: cargo build --tests --locked
  -t, --test      Test only: cargo test --locked (includes tests/e2e.rs)
  -c, --clippy    Clippy only: cargo clippy --all-targets --locked -- -D warnings
      --release   Build/test in release mode instead of debug
  -h, --help      Show this help

With no -b/-t/-c given, all three run (this is the default: same coverage
as CI's "Build, test, and lint" job).

tests/e2e.rs has a [dev-dependencies] path dependency on a sibling
../microtak-server checkout (it starts a real microtak_server::app::App
in-process) -- even a plain build needs that checkout present to resolve
the manifest, not just the test binary. See README.md "Development".
EOF
}

do_build=0
do_test=0
do_clippy=0
release=0

while [ $# -gt 0 ]; do
  case "$1" in
    -b|--build) do_build=1 ;;
    -t|--test) do_test=1 ;;
    -c|--clippy) do_clippy=1 ;;
    --release) release=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown option: $1" >&2; usage >&2; exit 1 ;;
  esac
  shift
done

if [ "$do_build" -eq 0 ] && [ "$do_test" -eq 0 ] && [ "$do_clippy" -eq 0 ]; then
  do_build=1; do_test=1; do_clippy=1
fi

if [ ! -d ../microtak-server ]; then
  echo "error: tests/e2e.rs depends on a sibling ../microtak-server checkout" >&2
  echo "  (a [dev-dependencies] path dependency), which is missing." >&2
  echo "  clone it next to this repo:" >&2
  echo "    git clone https://github.com/microtak/microtak-server ../microtak-server" >&2
  exit 1
fi

release_flag=()
[ "$release" -eq 1 ] && release_flag=(--release)

if [ "$do_build" -eq 1 ]; then
  echo "==> cargo build --tests --locked ${release_flag[*]}"
  cargo build --tests --locked "${release_flag[@]}"
fi

if [ "$do_test" -eq 1 ]; then
  echo "==> cargo test --locked ${release_flag[*]}"
  cargo test --locked "${release_flag[@]}"
fi

if [ "$do_clippy" -eq 1 ]; then
  echo "==> cargo clippy --all-targets --locked -- -D warnings"
  cargo clippy --all-targets --locked -- -D warnings
fi

echo "==> done"
