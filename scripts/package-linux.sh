#!/usr/bin/env bash
# Backward-compatible entry point. The implementation is now Python.
set -euo pipefail
exec python3 "$(cd "$(dirname "$0")" && pwd)/package_linux.py" "$@"
