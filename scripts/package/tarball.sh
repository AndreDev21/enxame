#!/usr/bin/env bash
# Empacota o CLI e o nó headless (binários de release do cargo) em um .tar.gz com .sha256.
# Uso: scripts/package/tarball.sh [--version x.y.z] [--out <dir>]
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
OUT=$ROOT/dist
while [[ $# -gt 0 ]]; do
  case $1 in
    --version) VERSION=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    *) echo "argumento desconhecido: $1" >&2; exit 2 ;;
  esac
done
cd "$ROOT"
cargo build --release --locked -p enxame-cli -p enxame-node
NAME=enxame-cli-$VERSION-x86_64-linux
W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
mkdir -p "$W/$NAME"
cp target/release/enxame-cli target/release/enxame-node "$W/$NAME/"
cp LICENSE README.md "$W/$NAME/"
mkdir -p "$OUT"
tar -C "$W" -czf "$OUT/$NAME.tar.gz" "$NAME"
( cd "$OUT" && sha256sum "$NAME.tar.gz" > "$NAME.tar.gz.sha256" )
ls -lh "$OUT/$NAME.tar.gz" | awk '{print $5, $9}'
