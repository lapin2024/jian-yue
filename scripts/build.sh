#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
case "${1:-release}" in
  release) cargo build --release --locked ;;
  test) cargo test --locked ;;
  check) cargo check --locked ;;
  *) echo 'Usage: scripts/build.sh [release|test|check]' >&2; exit 2 ;;
esac
