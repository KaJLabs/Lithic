#!/usr/bin/env python3
"""Read-only preflight for the unstarted, isolated LithoVM benchmark home.

This is a preparation guard, not a node launcher or validator acceptance test.
It deliberately rejects any signing/peer keys; a later synthetic-key phase
requires a separate, approved procedure.
"""

import argparse
import hashlib
import json
from pathlib import Path
import sys
import tomllib


EXPECTED_CHAIN_ID = "lithicbench_700778-1"
EXPECTED_BLOCK_GAS = 100_000_000
EXPECTED_BLOCK_BYTES = 21_000_000
EXPECTED_CANDIDATE_SHA256 = "2f34b8ed36c875698e20c6c6f7787fd1784b7351e85cc09b1d7a022593614e0e"
EXPECTED_APP_SHA256 = "5997f2546f926f7220de9152229e76beb98a1ff06c45d777cbedf22b8bd31350"
EXPECTED_COMET_SHA256 = "b60ab5d24fbd8fa8569b3b64ddcf8ada8a8dc1908a32050cc01716a2776ac08a"
EXPECTED_GENESIS_SHA256 = "b5012a1290c86f837e9eafe40f318e3866f1044eeb54680ac9bd3a4e461ea7e5"
SYNTHETIC_KEYS = (
    "config/priv_validator_key.json",
    "config/node_key.json",
    "data/priv_validator_state.json",
)


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def loopback(address: str) -> bool:
    return address.startswith("tcp://127.0.0.1:") or address.startswith("127.0.0.1:")


def check_home(home: Path, candidate: Path, *, strict_hashes: bool = True) -> dict:
    app_path = home / "config/app.toml"
    comet_path = home / "config/config.toml"
    genesis_path = home / "config/genesis.json"
    paths = {"candidate": candidate, "app": app_path, "comet": comet_path, "genesis": genesis_path}
    for label, path in paths.items():
        require(path.is_file(), f"{label} file missing")
        require(not path.is_symlink(), f"{label} file is a symlink")
    hashes = {label: digest(path) for label, path in paths.items()}
    if strict_hashes:
        for label, expected in {
            "candidate": EXPECTED_CANDIDATE_SHA256,
            "app": EXPECTED_APP_SHA256,
            "comet": EXPECTED_COMET_SHA256,
            "genesis": EXPECTED_GENESIS_SHA256,
        }.items():
            require(hashes[label] == expected, f"{label} SHA-256 mismatch")

    app = tomllib.loads(app_path.read_text(encoding="utf-8"))
    comet = tomllib.loads(comet_path.read_text(encoding="utf-8"))
    genesis = json.loads(genesis_path.read_text(encoding="utf-8"))
    require(genesis["chain_id"] == EXPECTED_CHAIN_ID, "unexpected or live chain ID")
    block = genesis["consensus"]["params"]["block"]
    require(int(block["max_gas"]) == EXPECTED_BLOCK_GAS, "unexpected block gas ceiling")
    require(int(block["max_bytes"]) == EXPECTED_BLOCK_BYTES, "unexpected block byte ceiling")
    require(not app["api"]["enable"], "API listener enabled")
    require(not app["grpc"]["enable"], "gRPC listener enabled")
    require(not app["grpc-web"]["enable"], "gRPC-web listener enabled")
    require(not app["json-rpc"]["enable"], "JSON-RPC listener enabled")
    require(loopback(app["json-rpc"]["address"]), "JSON-RPC is not loopback-bound")
    require(loopback(app["json-rpc"]["ws-address"]), "WebSocket is not loopback-bound")
    require(loopback(comet["p2p"]["laddr"]), "P2P is not loopback-bound")
    require(loopback(comet["rpc"]["laddr"]), "Comet RPC is not loopback-bound")
    require(comet["p2p"]["seeds"] == "", "live or external seeds configured")
    require(comet["p2p"]["persistent_peers"] == "", "persistent peers configured")
    require(comet["p2p"]["external_address"] == "", "external P2P address configured")
    require(not comet["p2p"]["pex"], "peer exchange enabled")
    require(comet["p2p"]["max_num_inbound_peers"] == 0, "inbound peers enabled")
    require(comet["p2p"]["max_num_outbound_peers"] == 0, "outbound peers enabled")
    for relative in SYNTHETIC_KEYS:
        require(not (home / relative).exists(), f"key/state file present: {relative}")
    return {"status": "safe_pre_key_baseline", "chain_id": EXPECTED_CHAIN_ID, "hashes": hashes}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--home", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    args = parser.parse_args()
    result = check_home(args.home, args.candidate)
    print(json.dumps(result, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (OSError, KeyError, ValueError, RuntimeError, tomllib.TOMLDecodeError) as error:
        print(f"isolated benchmark preflight failed: {error}", file=sys.stderr)
        raise SystemExit(1)
