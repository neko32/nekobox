#!/bin/sh
# nekobox backend entrypoint
# - Reads /nekobox/config/mcp_servers.json and installs listed MCP server packages via uv
# - Then starts the backend binary

set -e

CFG=/nekobox/config/mcp_servers.json

if [ -f "$CFG" ]; then
    echo "[nekobox] Reading MCP server list from $CFG"
    servers=$(jq -r 'if .servers then .servers[] else empty end' "$CFG" 2>/dev/null || true)
    for pkg in $servers; do
        [ -z "$pkg" ] && continue
        if uv tool list 2>/dev/null | grep -q "^$pkg "; then
            echo "[nekobox] MCP server already installed: $pkg"
        else
            echo "[nekobox] Installing MCP server: $pkg"
            uv tool install "$pkg" || echo "[nekobox] WARNING: Failed to install MCP server: $pkg"
        fi
    done
else
    echo "[nekobox] No mcp_servers.json found, skipping MCP server setup"
fi

exec /app/nekobox-backend "$@"
