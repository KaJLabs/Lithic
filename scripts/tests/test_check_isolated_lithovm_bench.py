"""Read-only safety checks for the isolated pre-key benchmark baseline."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check-isolated-lithovm-bench.py"
spec = importlib.util.spec_from_file_location("check_isolated_lithovm_bench", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PreflightTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.base = Path(self.directory.name)
        self.home = self.base / "home"
        (self.home / "config").mkdir(parents=True)
        self.candidate = self.base / "candidate"
        self.candidate.write_bytes(b"disabled candidate")
        self.app = self.home / "config/app.toml"
        self.comet = self.home / "config/config.toml"
        self.genesis = self.home / "config/genesis.json"
        self.app.write_text(
            '[api]\nenable = false\n[grpc]\nenable = false\n'
            '[grpc-web]\nenable = false\n[json-rpc]\nenable = false\n'
            'address = "127.0.0.1:8545"\nws-address = "127.0.0.1:8546"\n',
            encoding="utf-8",
        )
        self.comet.write_text(
            '[p2p]\nladdr = "tcp://127.0.0.1:26656"\n'
            'seeds = ""\npersistent_peers = ""\nexternal_address = ""\n'
            'pex = false\nmax_num_inbound_peers = 0\nmax_num_outbound_peers = 0\n'
            '[rpc]\nladdr = "tcp://127.0.0.1:26657"\n',
            encoding="utf-8",
        )
        self.genesis.write_text(json.dumps({
            "chain_id": module.EXPECTED_CHAIN_ID,
            "consensus": {"params": {"block": {
                "max_gas": str(module.EXPECTED_BLOCK_GAS),
                "max_bytes": str(module.EXPECTED_BLOCK_BYTES),
            }}},
        }), encoding="utf-8")

    def check(self):
        return module.check_home(self.home, self.candidate, strict_hashes=False)

    def test_safe_pre_key_baseline_passes(self):
        self.assertEqual(self.check()["status"], "safe_pre_key_baseline")

    def test_unpinned_candidate_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "candidate SHA-256 mismatch"):
            module.check_home(self.home, self.candidate)

    def test_live_chain_identity_is_rejected(self):
        value = json.loads(self.genesis.read_text(encoding="utf-8"))
        value["chain_id"] = "lithosphere_700777-2"
        self.genesis.write_text(json.dumps(value), encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "live chain ID"):
            self.check()

    def test_public_p2p_and_peer_seeds_are_rejected(self):
        self.comet.write_text(self.comet.read_text(encoding="utf-8").replace(
            "tcp://127.0.0.1:26656", "tcp://0.0.0.0:26656"), encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "P2P is not loopback"):
            self.check()
        self.comet.write_text(self.comet.read_text(encoding="utf-8").replace(
            "tcp://0.0.0.0:26656", "tcp://127.0.0.1:26656").replace(
            'seeds = ""', 'seeds = "live-peer"'), encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "seeds configured"):
            self.check()

    def test_public_rpc_is_rejected(self):
        self.app.write_text(self.app.read_text(encoding="utf-8").replace(
            'address = "127.0.0.1:8545"', 'address = "0.0.0.0:8545"'), encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "JSON-RPC is not loopback"):
            self.check()

    def test_key_and_wrong_ceiling_are_rejected(self):
        key = self.home / "config/priv_validator_key.json"
        key.write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "key/state file present"):
            self.check()
        key.unlink()
        value = json.loads(self.genesis.read_text(encoding="utf-8"))
        value["consensus"]["params"]["block"]["max_gas"] = "-1"
        self.genesis.write_text(json.dumps(value), encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "block gas ceiling"):
            self.check()


if __name__ == "__main__":
    unittest.main()
