"""Focused tests for the isolated Makalu config patcher."""

import importlib.util
from pathlib import Path
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "prepare-isolated-makalu-bench.py"
spec = importlib.util.spec_from_file_location("prepare_isolated_makalu_bench", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PatchTomlTests(unittest.TestCase):
    def test_changes_only_selected_keys(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "app.toml"
            path.write_text('pruning = "default"\nsecret = "keep"\n[evm]\nmax-tx-gas-wanted = 5\n', encoding="utf-8")
            module.patch_toml(path, {("", "pruning"): "custom", ("evm", "max-tx-gas-wanted"): 0})
            text = path.read_text(encoding="utf-8")
            self.assertIn('pruning = "custom"', text)
            self.assertIn('secret = "keep"', text)
            self.assertIn("max-tx-gas-wanted = 0", text)

    def test_missing_key_fails_without_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "app.toml"
            source = 'pruning = "default"\n'
            path.write_text(source, encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "Missing config keys"):
                module.patch_toml(path, {("evm", "max-tx-gas-wanted"): 0})
            self.assertEqual(path.read_text(encoding="utf-8"), source)

    def test_duplicate_key_fails_without_writing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "app.toml"
            source = 'pruning = "default"\npruning = "custom"\n'
            path.write_text(source, encoding="utf-8")
            with self.assertRaisesRegex(RuntimeError, "Duplicate config key"):
                module.patch_toml(path, {("", "pruning"): "custom"})
            self.assertEqual(path.read_text(encoding="utf-8"), source)

    def test_live_only_iavl_setting_is_inserted_at_top_level(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "app.toml"
            path.write_text('[api]\nenable = true\n', encoding="utf-8")
            module.patch_toml(path, {("", "iavl-lazy-loading"): False})
            self.assertTrue(path.read_text(encoding="utf-8").startswith("iavl-lazy-loading = false\n[api]"))


if __name__ == "__main__":
    unittest.main()
