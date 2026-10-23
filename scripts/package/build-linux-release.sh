#!/usr/bin/env bash
# Gera os artefatos de release para Linux (AppImage do app + tar.gz do CLI e do nó) dentro de um
# contêiner Ubuntu 22.04, para rodarem em distribuições dessa idade ou mais novas. Só precisa do
# Docker (sem BuildKit). Saída em ./dist (ou --out).
#
# Uso: scripts/package/build-linux-release.sh [--out <dir>] [--jobs <n>]
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
OUT=$ROOT/dist
JOBS=4
while [[ $# -gt 0 ]]; do
  case $1 in
    --out) OUT=$2; shift 2 ;;
    --jobs) JOBS=$2; shift 2 ;;
    *) echo "argumento desconhecido: $1" >&2; exit 2 ;;
  esac
done
IMAGE=enxame/release-build:local
cd "$ROOT"
docker build -f packaging/linux/Dockerfile.build --target build \
  --build-arg CARGO_BUILD_JOBS="$JOBS" -t "$IMAGE" .
CID=$(docker create "$IMAGE")
trap 'docker rm -f "$CID" >/dev/null 2>&1 || true' EXIT
mkdir -p "$OUT"
docker cp "$CID:/out/." "$OUT/"
ls -lh "$OUT"
