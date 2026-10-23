#!/usr/bin/env bash
# Monta o AppImage do Enxame a partir do bundle do Flutter (`flutter build linux --release`).
#
# Uso: scripts/package/appimage.sh [--bundle <dir>] [--version <x.y.z>] [--out <dir>]
# Saída: <out>/Enxame-<versão>-x86_64.AppImage e o .sha256.
#
# O AppImage usa o GTK 3, o D-Bus e o PulseAudio do sistema (todo desktop os tem); para a maior
# compatibilidade com distribuições antigas, compile em Ubuntu 22.04 (packaging/linux/Dockerfile.build).
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/../.." && pwd)
BUNDLE=$ROOT/app/build/linux/x64/release/bundle
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
OUT=$ROOT/dist
while [[ $# -gt 0 ]]; do
  case $1 in
    --bundle) BUNDLE=$2; shift 2 ;;
    --version) VERSION=$2; shift 2 ;;
    --out) OUT=$2; shift 2 ;;
    *) echo "argumento desconhecido: $1" >&2; exit 2 ;;
  esac
done
[[ -x "$BUNDLE/enxame_app" ]] || { echo "falta o bundle: rode 'flutter build linux --release' em app/" >&2; exit 1; }
[[ -f "$BUNDLE/lib/libenxame_ffi.so" ]] || { echo "o bundle não tem libenxame_ffi.so" >&2; exit 1; }

# appimagetool fixado e verificado (cadeia de suprimentos).
TOOL_VERSION=1.9.1
TOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
TOOLS=${APPIMAGE_TOOLS_DIR:-$ROOT/target/appimage-tools}
TOOL=$TOOLS/appimagetool-$TOOL_VERSION-x86_64.AppImage
if [[ ! -x "$TOOL" ]]; then
  mkdir -p "$TOOLS"
  curl -fsSL -o "$TOOL" \
    "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-x86_64.AppImage"
  echo "$TOOL_SHA256  $TOOL" | sha256sum -c - >/dev/null || { rm -f "$TOOL"; echo "sha256 do appimagetool não confere" >&2; exit 1; }
  chmod +x "$TOOL"
fi

# Runtime do AppImage (a parte que monta o squashfs ao abrir), também fixado e verificado.
RUNTIME_TAG=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
RUNTIME=$TOOLS/runtime-$RUNTIME_TAG-x86_64
if [[ ! -f "$RUNTIME" ]]; then
  curl -fsSL -o "$RUNTIME" \
    "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_TAG/runtime-x86_64"
  echo "$RUNTIME_SHA256  $RUNTIME" | sha256sum -c - >/dev/null || { rm -f "$RUNTIME"; echo "sha256 do runtime não confere" >&2; exit 1; }
fi

W=$(mktemp -d)
trap 'rm -rf "$W"' EXIT
APPDIR=$W/Enxame.AppDir
mkdir -p "$APPDIR/usr/lib/enxame"
cp -a "$BUNDLE/." "$APPDIR/usr/lib/enxame/"
cp "$ROOT/packaging/linux/enxame.desktop" "$APPDIR/enxame.desktop"
cp "$ROOT/packaging/linux/enxame.png" "$APPDIR/enxame.png"
cp "$ROOT/packaging/linux/enxame.png" "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
# $APPDIR e $APPIMAGE vêm do runtime do AppImage; o app usa $APPIMAGE para registrar o
# "abrir com o sistema" (o caminho dentro do ponto de montagem muda a cada execução).
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/lib/enxame/enxame_app" "$@"
RUN
chmod +x "$APPDIR/AppRun"

mkdir -p "$OUT"
FILE=$OUT/Enxame-$VERSION-x86_64.AppImage
# --appimage-extract-and-run: não exige FUSE no ambiente de build (contêiner, CI).
ARCH=x86_64 "$TOOL" --appimage-extract-and-run --no-appstream --runtime-file "$RUNTIME" "$APPDIR" "$FILE" >/dev/null
chmod +x "$FILE"
( cd "$OUT" && sha256sum "$(basename "$FILE")" > "$(basename "$FILE").sha256" )
echo "$FILE"
ls -lh "$FILE" | awk '{print $5, $9}'
