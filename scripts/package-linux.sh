#!/usr/bin/env bash
# Build .deb and .rpm for Cocktail Manager (Linux / WSL).
# Requires: cargo, npm, nfpm (https://nfpm.goreleaser.com/)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="${VERSION:-$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')}"
ARCH_RAW="$(uname -m)"
case "$ARCH_RAW" in
  x86_64|amd64) ARCH=amd64; RPM_ARCH=x86_64 ;;
  aarch64|arm64) ARCH=arm64; RPM_ARCH=aarch64 ;;
  *) ARCH="$ARCH_RAW"; RPM_ARCH="$ARCH_RAW" ;;
esac

echo "==> version=$VERSION arch=$ARCH"

echo "==> cargo build --release"
cargo build -p cocktail-control --release

echo "==> admin npm build"
(cd admin && npm ci && npm run build)

STAGE="$ROOT/dist/stage"
rm -rf "$STAGE" "$ROOT/dist"/*.deb "$ROOT/dist"/*.rpm 2>/dev/null || true
mkdir -p "$STAGE/usr/bin" "$STAGE/usr/share/cocktail/web"

cp -f target/release/cocktail-control "$STAGE/usr/bin/cocktail-control"
chmod 755 "$STAGE/usr/bin/cocktail-control"
cp -a admin/dist/. "$STAGE/usr/share/cocktail/web/"

if ! command -v nfpm >/dev/null 2>&1; then
  echo "nfpm not found - installing to ./dist/tools ..."
  mkdir -p dist/tools
  # Goreleaser assets: Linux_x86_64 / Linux_arm64 (capital L; uname -m aarch64 -> arm64)
  case "$ARCH_RAW" in
    x86_64|amd64) NFPM_ARCH=x86_64 ;;
    aarch64|arm64) NFPM_ARCH=arm64 ;;
    *) NFPM_ARCH="$ARCH_RAW" ;;
  esac
  NFPM_VER=2.41.3
  NFPM_URL="https://github.com/goreleaser/nfpm/releases/download/v${NFPM_VER}/nfpm_${NFPM_VER}_Linux_${NFPM_ARCH}.tar.gz"
  echo "==> downloading nfpm: $NFPM_URL"
  curl -fsSL "$NFPM_URL" | tar -xz -C dist/tools nfpm
  export PATH="$ROOT/dist/tools:$PATH"
fi

export VERSION ARCH
# nfpm uses ${VERSION} ${ARCH} from env when using -f with replacements - inject via sed
TMP_NFPM="$(mktemp)"
sed -e "s/\${VERSION}/$VERSION/g" -e "s/\${ARCH}/$ARCH/g" packaging/nfpm.yaml > "$TMP_NFPM"

mkdir -p dist
nfpm package -f "$TMP_NFPM" -p deb -t dist/
# rpm arch naming
sed -e "s/\${VERSION}/$VERSION/g" -e "s/\${ARCH}/$RPM_ARCH/g" packaging/nfpm.yaml > "$TMP_NFPM"
nfpm package -f "$TMP_NFPM" -p rpm -t dist/
rm -f "$TMP_NFPM"

echo "==> packages:"
ls -la dist/*."deb" dist/*."rpm" 2>/dev/null || ls -la dist/
