#!/usr/bin/env bash
set -euo pipefail
export PATH="${HOME}/.cargo/bin:/usr/bin:/bin:${PATH}"
SRC=/mnt/d/Cocktail
BUILD="${HOME}/cocktail-linux-build"
export CARGO_TARGET_DIR="${HOME}/cocktail-linux-target"
mkdir -p "${BUILD}" "${CARGO_TARGET_DIR}"
echo "host=$(uname -m) rustc=$(rustc -V) cargo=$(command -v cargo)"
echo "syncing sources to ${BUILD}"
if command -v rsync >/dev/null 2>&1; then
  rsync -a --delete \
    --exclude target/ \
    --exclude admin/node_modules/ \
    --exclude admin/dist/ \
    --exclude data/ \
    --exclude .git/ \
    --exclude dist/ \
    "${SRC}/" "${BUILD}/"
else
  tar -C "${SRC}" --exclude=target --exclude=admin/node_modules --exclude=admin/dist --exclude=data --exclude=.git --exclude=dist -cf - . \
    | tar -C "${BUILD}" -xf -
fi
cd "${BUILD}"
echo "building in $(pwd) target=${CARGO_TARGET_DIR}"
cargo build --release -p cocktail-control --bins
OUT="${SRC}/dist/linux-x64"
mkdir -p "${OUT}"
cp -f "${CARGO_TARGET_DIR}/release/cocktail-control" "${CARGO_TARGET_DIR}/release/cocktail-agent" "${OUT}/"
chmod +x "${OUT}/cocktail-control" "${OUT}/cocktail-agent"
echo "artifacts:"
ls -lh "${OUT}/cocktail-control" "${OUT}/cocktail-agent"
file "${OUT}/cocktail-control" "${OUT}/cocktail-agent" || true
