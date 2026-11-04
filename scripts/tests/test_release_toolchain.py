import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('release_toolchain', Path(__file__).parents[1] / 'release_toolchain.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def source_repo(self, directory):
        root = Path(directory) / 'source'
        root.mkdir()
        subprocess.run(['git', 'init', str(root)], check=True, capture_output=True)
        for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'):
            (root / name).write_bytes(b'canonical = "input"\n')
        (root / 'fixture.bin').write_bytes(b'\x00binary\r\n')
        return root

    def commit_source(self, root):
        subprocess.run(['git', '-c', 'core.autocrlf=false', 'add', '.'],
                       cwd=root, check=True, capture_output=True)
        subprocess.run(['git', '-c', 'user.name=Release Test',
                        '-c', 'user.email=release-test@example.invalid',
                        '-c', 'commit.gpgsign=false', 'commit', '-m', 'fixture'],
                       cwd=root, check=True, capture_output=True)

    def test_clean_crlf_checkout_cannot_claim_raw_git_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.source_repo(directory)
            self.commit_source(root)
            clone = Path(directory) / 'transformed-checkout'
            subprocess.run(['git', '-c', 'core.autocrlf=true', 'clone', str(root), str(clone)],
                           check=True, capture_output=True)
            self.assertEqual((clone / 'Cargo.lock').read_bytes(), b'canonical = "input"\r\n')
            self.assertEqual(release.run(['git', 'status', '--porcelain'], cwd=clone), b'')
            with self.assertRaisesRegex(ValueError, 'noncanonical source bytes: Cargo.lock'):
                release.source_snapshot(clone)

    def test_raw_git_source_policy_preserves_binary_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.source_repo(directory)
            self.commit_source(root)
            source = release.source_snapshot(root)
            self.assertEqual(source['fixture.bin'], b'\x00binary\r\n')
            self.assertEqual(source['Cargo.lock'], b'canonical = "input"\n')

    def test_lf_checkout_policy_overrides_autocrlf_and_preserves_fuzz_inputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = self.source_repo(directory)
            (root / '.gitattributes').write_bytes((release.ROOT / '.gitattributes').read_bytes())
            fuzz = root / 'fuzz/corpus/child_creation/seed'
            fuzz.parent.mkdir(parents=True)
            fuzz.write_bytes(b'fuzz bytes\r\n')
            self.commit_source(root)
            clone = Path(directory) / 'windows-checkout'
            subprocess.run(['git', '-c', 'core.autocrlf=true', 'clone', str(root), str(clone)],
                           check=True, capture_output=True)
            self.assertEqual(release.source_snapshot(root), release.source_snapshot(clone))
            self.assertEqual((clone / 'Cargo.lock').read_bytes(), b'canonical = "input"\n')
            self.assertEqual((clone / 'fuzz/corpus/child_creation/seed').read_bytes(), b'fuzz bytes\r\n')

    def test_native_link_metadata_settings_are_platform_specific(self):
        root=Path('/source')
        linux=release.release_rustflags('x86_64-unknown-linux-gnu',root).split('\x1f')
        windows=release.release_rustflags('x86_64-pc-windows-msvc',root).split('\x1f')
        self.assertEqual(linux,['--remap-path-prefix='+str(root)+'=/lithic'])
        self.assertEqual(windows,linux+['-C','link-arg=/Brepro'])
        self.assertEqual(release.native_library_link_args('aarch64-apple-darwin'),
                         ['-C','link-arg=-Wl,-install_name,@rpath/liblithovm_ffi.dylib'])
        self.assertEqual(release.native_library_link_args('x86_64-unknown-linux-gnu'),[])

    def test_mismatch_preserves_actual_payloads_and_never_emits_candidate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / 'candidate'
            builds = [{'bin/lithc': b'first', 'lib/runtime': b'same'},
                      {'bin/lithc': b'second', 'lib/runtime': b'same'}]
            release.preserve_rebuild_failure(builds, [root/'one', root/'two'],output,'linux','test-commit')
            diagnostic = root/'candidate-failed-rebuild'
            self.assertFalse(output.exists())
            self.assertEqual((diagnostic/'build-0/bin/lithc').read_bytes(),b'first')
            self.assertEqual((diagnostic/'build-1/bin/lithc').read_bytes(),b'second')
            record=json.loads((diagnostic/'FAILED_REBUILD.json').read_bytes())
            self.assertEqual(record['status'],'FAILED_REBUILD_NOT_A_RELEASE')
            self.assertEqual(set(record['differences']),{'bin/lithc'})

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
        for mutation in ('runtime', 'verifier', 'devtools', 'modified', 'extra'):
            with self.subTest(mutation=mutation):
                files = self.files('x86_64-unknown-linux-gnu')
                if mutation in ('runtime', 'verifier', 'devtools'):
                    name = {'runtime': 'lib/liblithovm_ffi.so', 'verifier': 'bin/lithverify',
                            'devtools': 'bin/lithdev'}[mutation]
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
