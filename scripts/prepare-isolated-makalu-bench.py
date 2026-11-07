#!/usr/bin/env python3
"""Prepare a non-networked Makalu baseline config on the dedicated bench VPS.

This creates synthetic config from the exact live binary, mirrors allowlisted
execution settings, and removes the synthetic keys emitted by `lithod init`.
It never starts a node or copies live config, keys, state, or peers.
"""

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


BASE = Path("/var/lib/lithic-bench")
BINARY = BASE / "source/lithod-l1-v20.0.0-r1"
HOME = BASE / "makalu-isolated"
EXPECTED_BINARY_SHA256 = "1f03146df86391715b86971b14b6074580b7efd06d7265a1725d90e426b8efbc"
CHAIN_ID = "lithicbench_700778-1"

APP_VALUES = {
    ("", "app-db-backend"): "",
    ("", "iavl-cache-size"): 781250,
    ("", "iavl-disable-fastnode"): False,
    ("", "iavl-lazy-loading"): False,
    ("", "inter-block-cache"): True,
    ("", "minimum-gas-prices"): "0ulitho",
    ("", "pruning"): "custom",
    ("", "pruning-keep-recent"): "100",
    ("", "pruning-interval"): "10",
    ("evm", "max-tx-gas-wanted"): 0,
    ("json-rpc", "gas-cap"): 25000000,
    ("state-sync", "snapshot-interval"): 1000,
    ("state-sync", "snapshot-keep-recent"): 2,
    # Isolation overrides: no API listener, even on loopback, until reviewed.
    ("api", "enable"): False,
    ("grpc", "enable"): False,
    ("grpc-web", "enable"): False,
    ("json-rpc", "enable"): False,
    ("json-rpc", "address"): "127.0.0.1:8545",
    ("json-rpc", "ws-address"): "127.0.0.1:8546",
}

COMET_VALUES = {
    ("", "db_backend"): "goleveldb",
    ("blocksync", "version"): "v0",
    ("consensus", "create_empty_blocks"): True,
    ("consensus", "timeout_commit"): "900ms",
    ("consensus", "timeout_propose"): "3s",
    ("mempool", "max_tx_bytes"): 1048576,
    ("mempool", "max_txs_bytes"): 1073741824,
    ("mempool", "size"): 5000,
    ("statesync", "enable"): False,
    ("storage", "discard_abci_responses"): False,
    ("tx_index", "indexer"): "kv",
    # Isolation overrides. Distinct chain identity is set in genesis below.
    ("p2p", "laddr"): "tcp://127.0.0.1:26656",
    ("p2p", "external_address"): "",
    ("p2p", "seeds"): "",
    ("p2p", "persistent_peers"): "",
    ("p2p", "pex"): False,
    ("p2p", "max_num_inbound_peers"): 0,
    ("p2p", "max_num_outbound_peers"): 0,
    ("rpc", "laddr"): "tcp://127.0.0.1:26657",
    ("rpc", "grpc_laddr"): "",
    ("rpc", "pprof_laddr"): "",
    ("rpc", "unsafe"): False,
}

SYNTHETIC_KEYS = (
    "config/priv_validator_key.json",
    "config/node_key.json",
    "data/priv_validator_state.json",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def toml_literal(value: bool | int | str) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    return json.dumps(value)


def lookup(data: dict, section: str, key: str):
    return data[section][key] if section else data[key]


def patch_toml(path: Path, values: dict[tuple[str, str], bool | int | str]) -> None:
    source = path.read_text(encoding="utf-8")
    section = ""
    seen = set()
    output = []
    for line in source.splitlines(keepends=True):
        header = re.match(r"^\s*\[([^]]+)\]\s*(?:#.*)?$", line.strip())
        if header:
            section = header.group(1)
        assignment = re.match(r"^(\s*)([A-Za-z0-9_-]+)\s*=", line)
        if assignment:
            pair = (section, assignment.group(2))
            if pair in values:
                if pair in seen:
                    raise RuntimeError(f"Duplicate config key: {pair}")
                output.append(f"{assignment.group(1)}{pair[1]} = {toml_literal(values[pair])}\n")
                seen.add(pair)
                continue
        output.append(line)
    missing = values.keys() - seen
    # This setting is present on the live Makalu node but absent from the
    # binary's generated default app.toml; adding it is an intentional overlay.
    optional_insertions = {("", "iavl-lazy-loading")} if path.name == "app.toml" else set()
    if missing - optional_insertions:
        raise RuntimeError(f"Missing config keys in {path.name}: {sorted(missing - optional_insertions)}")
    for section, key in sorted(missing):
        if section != "":
            raise RuntimeError(f"Cannot insert non-top-level config key: {section}.{key}")
        output.insert(0, f"{key} = {toml_literal(values[(section, key)])}\n")
    patched = "".join(output)
    parsed = tomllib.loads(patched)
    for (section, key), expected in values.items():
        if lookup(parsed, section, key) != expected:
            raise RuntimeError(f"Config verification failed: {section}.{key}")
    path.write_text(patched, encoding="utf-8")


def remove_synthetic_keys() -> None:
    for relative in SYNTHETIC_KEYS:
        target = HOME / relative
        if target.is_file():
            target.unlink()


def main() -> None:
    if os.geteuid() == 0:
        raise RuntimeError("Run as the unprivileged lithicbench user, never root")
    if Path.home() != Path("/home/lithicbench"):
        raise RuntimeError("Unexpected account home")
    if sha256(BINARY) != EXPECTED_BINARY_SHA256:
        raise RuntimeError("Makalu binary is not the captured running binary")
    if HOME.exists():
        raise RuntimeError(f"Refusing to overwrite existing benchmark home: {HOME}")
    if BASE.resolve() != Path("/var/lib/lithic-bench"):
        raise RuntimeError("Unexpected benchmark base path")

    try:
        subprocess.run(
            [str(BINARY), "init", "lithic-bench-01", "--chain-id", CHAIN_ID,
             "--home", str(HOME)],
            check=True, timeout=60, stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        config_dir = HOME / "config"
        patch_toml(config_dir / "app.toml", APP_VALUES)
        patch_toml(config_dir / "config.toml", COMET_VALUES)
        genesis_path = config_dir / "genesis.json"
        genesis = json.loads(genesis_path.read_text(encoding="utf-8"))
        genesis["chain_id"] = CHAIN_ID
        genesis["consensus"]["params"]["block"]["max_gas"] = "100000000"
        genesis["consensus"]["params"]["block"]["max_bytes"] = "21000000"
        genesis_path.write_text(json.dumps(genesis, sort_keys=True, indent=2) + "\n", encoding="utf-8")
        if json.loads(genesis_path.read_text(encoding="utf-8"))["chain_id"] != CHAIN_ID:
            raise RuntimeError("Benchmark chain identity verification failed")
    finally:
        # `init` emits fresh local keys; they must never remain in this home.
        remove_synthetic_keys()

    for relative in SYNTHETIC_KEYS:
        if (HOME / relative).exists():
            raise RuntimeError(f"Synthetic key removal failed: {relative}")
    print(json.dumps({
        "home": str(HOME),
        "binary_sha256": EXPECTED_BINARY_SHA256,
        "chain_id": CHAIN_ID,
        "app_sha256": sha256(HOME / "config/app.toml"),
        "comet_sha256": sha256(HOME / "config/config.toml"),
        "genesis_sha256": sha256(HOME / "config/genesis.json"),
        "synthetic_keys_present": False,
        "node_started": False,
    }, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"benchmark config preparation failed: {error}", file=sys.stderr)
        raise SystemExit(1)
