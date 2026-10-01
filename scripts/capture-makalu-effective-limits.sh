#!/usr/bin/env bash
# Read-only capture on the running Makalu testnet node. Never reads key contents.
set -euo pipefail

LITHIC_SERVICE="${1:-lithod-mtest-val-02}"
LITHIC_NODE_HOME="${2:-/var/lib/litho-mtest-val-02}"
LITHIC_COMET_RPC="${3:-http://127.0.0.1:26757}"
LITHIC_EVM_RPC="${4:-http://127.0.0.1:8645}"

command -v jq >/dev/null || { echo 'jq is required' >&2; exit 2; }
command -v curl >/dev/null || { echo 'curl is required' >&2; exit 2; }
LITHIC_PID="$(systemctl show "$LITHIC_SERVICE" --property=MainPID --value)"
if [[ ! "$LITHIC_PID" =~ ^[1-9][0-9]*$ ]]; then
    echo "Service $LITHIC_SERVICE is not running" >&2
    exit 2
fi

printf 'observed_utc=%s\nservice=%s\npid=%s\n' "$(date -u +%FT%TZ)" "$LITHIC_SERVICE" "$LITHIC_PID"
printf 'binary_path=%s\n' "$(readlink -f "/proc/$LITHIC_PID/exe")"
sha256sum "/proc/$LITHIC_PID/exe"
"/proc/$LITHIC_PID/exe" version

for LITHIC_CONFIG_NAME in app.toml config.toml genesis.json; do
    LITHIC_CONFIG_PATH="$LITHIC_NODE_HOME/config/$LITHIC_CONFIG_NAME"
    if [[ -f "$LITHIC_CONFIG_PATH" ]]; then
        sha256sum "$LITHIC_CONFIG_PATH"
    else
        printf 'missing_config=%s\n' "$LITHIC_CONFIG_PATH"
    fi
done

curl --fail --silent --show-error --max-time 8 "$LITHIC_COMET_RPC/status" |
    jq -c '{network:.result.node_info.network, height:.result.sync_info.latest_block_height, block_time:.result.sync_info.latest_block_time, catching_up:.result.sync_info.catching_up}'
curl --fail --silent --show-error --max-time 8 "$LITHIC_COMET_RPC/consensus_params" |
    jq -c '{height:.result.block_height, block:.result.consensus_params.block}'

LITHIC_EVM_REQUEST='{"jsonrpc":"2.0","id":1,"method":"eth_getBlockByNumber","params":["latest",false]}'
if LITHIC_EVM_RESPONSE="$(curl --fail --silent --show-error --max-time 8 -H 'Content-Type: application/json' -d "$LITHIC_EVM_REQUEST" "$LITHIC_EVM_RPC")"; then
    jq -c '{evm_block:.result.number, evm_gas_limit:.result.gasLimit, evm_block_hash:.result.hash, error:.error}' <<< "$LITHIC_EVM_RESPONSE"
else
    echo 'evm_rpc_unavailable=true'
fi

# Only these non-secret application settings are printed; no service Environment,
# full process command line, full TOML, node_key, validator key or mnemonic.
LITHIC_APP_CONFIG="$LITHIC_NODE_HOME/config/app.toml"
if [[ -f "$LITHIC_APP_CONFIG" ]]; then
    grep -E '^[[:space:]]*(minimum-gas-prices|pruning|pruning-keep-recent|pruning-interval|snapshot-interval)[[:space:]]*=' "$LITHIC_APP_CONFIG" || true
fi
