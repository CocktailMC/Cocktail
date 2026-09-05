#!/bin/sh
set -e
systemctl stop cocktail-control.service >/dev/null 2>&1 || true
systemctl disable cocktail-control.service >/dev/null 2>&1 || true
