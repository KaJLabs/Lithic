import importlib.util
import io
import json
from pathlib import Path
import tarfile
import unittest

spec = importlib.util.spec_from_file_location('release_toolchain', Path(__file__).parents[1] / 'release_toolchain.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def files(self, target):
        names = set(release.artifact_names(target).values()) | {
            'include/lithovm.h', 'sbom.cdx.json', 'Cargo.lock', 'source-manifest.json',
            'build-evidence.json', 'smoke-evidence.json'}
        files = {n: n.encode() for n in names}
        files['manifest.json'] = release.canonical({'schema_version': 1, 'version': 'lithic-vtest',
            'target': target, 'files': {n: {'sha256': release.sha(raw), 'bytes': len(raw)}
                                       for n, raw in files.items()}})
        return files

    def test_archive_deterministic_and_complete_for_all_release_targets(self):
        for target in ('x86_64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc'):
            with self.subTest(target=target):
                files = self.files(target)
                first = release.archive_bytes(files, 1700000000, windows='windows' in target)
                second = release.archive_bytes(dict(reversed(list(files.items()))), 1700000000,
                                               windows='windows' in target)
                self.assertEqual(first, second)
                self.assertEqual(release.verify_archive(first)['target'], target)

    def test_missing_runtime_or_verifier_and_modified_payload_rejected(self):
        for mutation in ('runtime', 'verifier', 'modified', 'extra'):
            with self.subTest(mutation=mutation):
                files = self.files('x86_64-unknown-linux-gnu')
                if mutation in ('runtime', 'verifier'):
                    name = 'lib/liblithovm_ffi.so' if mutation == 'runtime' else 'bin/lithverify'
                    del files[name]
                    manifest = json.loads(files['manifest.json'])
                    del manifest['files'][name]
                    files['manifest.json'] = release.canonical(manifest)
                elif mutation == 'modified':
                    files['bin/lithc'] += b'changed'
                else:
                    files['extra'] = b'unlisted'
                with self.assertRaises(ValueError):
                    release.verify_archive(release.archive_bytes(files, 1700000000))

    def test_traversal_symlink_and_duplicate_archive_members_rejected(self):
        for name in ('../file', '/root', 'a/../b', 'a\\b', 'C:/file', 'a//b'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                release.safe_name(name)
        for kind in ('symlink', 'duplicate'):
            buffer = io.BytesIO()
            with tarfile.open(fileobj=buffer, mode='w:gz') as archive:
                member = tarfile.TarInfo('manifest.json')
                if kind == 'symlink':
                    member.type, member.linkname = tarfile.SYMTYPE, '/etc/passwd'
                    archive.addfile(member)
                else:
                    archive.addfile(member, io.BytesIO())
                    archive.addfile(member, io.BytesIO())
            with self.assertRaises(ValueError):
                release.verify_archive(buffer.getvalue())

    def test_sbom_keeps_dependency_edges_checksums_and_declared_licenses(self):
        packages = [{'id': 'root-id', 'name': 'lithc', 'version': '0.2.0', 'source': None, 'license': 'Apache-2.0'},
                    {'id': 'serde-id', 'name': 'serde', 'version': '1.0.0', 'source': 'registry+example',
                     'license': 'MIT OR Apache-2.0'}]
        metadata = {'packages': packages, 'resolve': {'nodes': [
            {'id': 'root-id', 'dependencies': ['serde-id']}, {'id': 'serde-id', 'dependencies': []}]}}
        lock = {'package': [{'name': 'serde', 'version': '1.0.0', 'source': 'registry+example', 'checksum': 'a' * 64}]}
        bom = release.sbom(metadata, lock)
        self.assertEqual(bom['bomFormat'], 'CycloneDX')
        self.assertEqual(bom['dependencies'][0]['dependsOn'], ['serde@1.0.0#registry+example'])
        self.assertEqual(bom['components'][1]['hashes'][0]['content'], 'a' * 64)
        self.assertNotIn('root-id', json.dumps(bom))


if __name__ == '__main__':
    unittest.main()
