"""Determinism and secret-exclusion tests for the remediation source bundle."""

import hashlib
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
import zipfile


SCRIPT = Path(__file__).resolve().parents[1] / "package-lithovm-remediation-review.py"
spec = importlib.util.spec_from_file_location("package_lithovm_remediation_review", SCRIPT)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def git(repository, *args):
    return subprocess.check_output(["git", "-C", str(repository), *args], text=True).strip()


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.base = Path(self.directory.name)
        self.patch = self.base / "sdk-compat.patch"
        self.patch.write_bytes(b"pinned fixture patch\n")
        original_digest = module.SDK_COMPAT_SHA256
        module.SDK_COMPAT_SHA256 = hashlib.sha256(self.patch.read_bytes()).hexdigest()
        self.addCleanup(setattr, module, "SDK_COMPAT_SHA256", original_digest)
        self.repositories = {}
        for name in module.SOURCES:
            repository = self.base / name
            repository.mkdir()
            subprocess.run(["git", "init", "-q", str(repository)], check=True)
            (repository / "source.txt").write_text(name + "\n", encoding="utf-8")
            if name == "evmos":
                (repository / "scripts").mkdir()
                (repository / "scripts/.env").write_text("do not package\n", encoding="utf-8")
            git(repository, "add", ".")
            subprocess.run(["git", "-C", str(repository), "-c", "user.name=Test",
                            "-c", "user.email=test@example.invalid", "commit", "-qm", "fixture"], check=True)
            self.repositories[name] = repository
            git(repository, "config", "core.autocrlf", "true")
        self.refs = {name: "HEAD" for name in module.SOURCES}

    def test_deterministic_forward_slash_source_and_checksums(self):
        first = self.base / "one.zip"
        second = self.base / "two.zip"
        result = module.package(first, self.repositories, self.refs, self.patch)
        module.package(second, self.repositories, self.refs, self.patch)
        self.assertEqual(first.read_bytes(), second.read_bytes())
        self.assertEqual(result["omitted"], ["evmos/scripts/.env"])
        with zipfile.ZipFile(first) as archive:
            names = archive.namelist()
            self.assertEqual(names, sorted(names))
            self.assertTrue(all("\\" not in name for name in names))
            self.assertNotIn("source/litho-native-chain-lab/scripts/.env", names)
            raw = subprocess.check_output(["git", "-C", str(self.repositories["lithic"]),
                                           "show", "HEAD:source.txt"])
            self.assertEqual(archive.read("source/lithic-toolchain/source.txt"), raw)
            self.assertEqual(archive.read("patches/cosmos-sdk-v0.50.14-evmos-compat.patch"),
                             self.patch.read_bytes())
            for line in archive.read("CHECKSUMS.sha256").decode().splitlines():
                digest, name = line.split("  ", 1)
                self.assertEqual(hashlib.sha256(archive.read(name)).hexdigest(), digest)

    def test_refuses_unexpected_tracked_environment_file(self):
        repository = self.repositories["sdk"]
        (repository / ".env").write_text("unsafe\n", encoding="utf-8")
        git(repository, "add", ".env")
        subprocess.run(["git", "-C", str(repository), "-c", "user.name=Test",
                        "-c", "user.email=test@example.invalid", "commit", "-qm", "unsafe"], check=True)
        with self.assertRaisesRegex(RuntimeError, "unexpected sensitive tracked path"):
            module.package(self.base / "rejected.zip", self.repositories, self.refs, self.patch)

    def test_rejects_wrong_sdk_patch_digest(self):
        self.patch.write_bytes(b"different patch\n")
        with self.assertRaisesRegex(RuntimeError, "SDK compatibility patch digest mismatch"):
            module.package(self.base / "rejected.zip", self.repositories, self.refs, self.patch)


if __name__ == "__main__":
    unittest.main()
