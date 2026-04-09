#!/bin/sh
# nekobox backend entrypoint
# - Reads /nekobox/config/mcp_servers.json and installs listed MCP server packages via uv
# - mcp_servers.json format:
#     { "servers": [ { "name": "<pkg>", "source": "<install-source>" }, ... ] }
#   "name"   : package name used to check uv tool list
#   "source" : passed to `uv tool install` (PyPI name or git+https://... URL)
# - Then starts the backend binary

set -e

CFG=/nekobox/config/mcp_servers.json

if [ -f "$CFG" ]; then
    echo "[nekobox] Reading MCP server list from $CFG"
    server_count=$(jq '.servers | length' "$CFG" 2>/dev/null || echo 0)
    i=0
    while [ "$i" -lt "$server_count" ]; do
        name=$(jq -r ".servers[$i].name" "$CFG")
        source=$(jq -r ".servers[$i].source // .servers[$i].name" "$CFG")
        i=$((i + 1))

        [ -z "$name" ] || [ "$name" = "null" ] && continue

        if uv tool list 2>/dev/null | grep -q "^${name} "; then
            echo "[nekobox] MCP server already installed: $name"
        else
            echo "[nekobox] Installing MCP server: $name (source: $source)"
            uv tool install "$source" || echo "[nekobox] WARNING: Failed to install MCP server: $name"
        fi
    done
else
    echo "[nekobox] No mcp_servers.json found, skipping MCP server setup"
fi

exec /app/nekobox-backend "$@"
