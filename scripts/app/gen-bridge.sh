#!/usr/bin/env bash
# Regenera a ponte Rust ↔ Dart (flutter_rust_bridge 2.13.0) a partir de crates/enxame-ffi/src/api.
# Instalação do codegen: cargo install flutter_rust_bridge_codegen --version '=2.13.0' --locked
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
command -v flutter_rust_bridge_codegen >/dev/null || {
  echo "falta o flutter_rust_bridge_codegen 2.13.0 (veja o cabeçalho deste script)" >&2; exit 1; }
exec flutter_rust_bridge_codegen generate
