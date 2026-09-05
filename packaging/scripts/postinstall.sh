#!/bin/sh
set -e
systemctl daemon-reload >/dev/null 2>&1 || true
systemctl enable cocktail-control.service >/dev/null 2>&1 || true
echo "Cocktail Manager installed. Start with: systemctl start cocktail-control"
echo "UI/API default: http://127.0.0.1:11011"
