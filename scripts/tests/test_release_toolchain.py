import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('release_toolchain', Path(__file__).parents[1] / 'release_toolchain.py')
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def cargo_messages(self, root, target='x86_64-unknown-linux-gnu'):
        rows = []
        for name, manifest in release.RELEASE_COMPONENTS.items():
            kind = ['cdylib', 'rlib'] if name == 'lithovm_ffi' else ['bin'] if name in release.TOOLS else ['lib']
            rows.append({'reason': 'compiler-artifact', 'package_id': 'path+file://' + str(root / manifest),
                         'manifest_path': str(root / manifest),
                         'target': {'name': name, 'kind': kind},
                         'profile': {'opt_level': '3', 'debuginfo': 0, 'debug_assertions': False,
                                     'overflow_checks': False, 'test': False},
                         'filenames': [str(root / 'target' / target / 'release' / 'deps' / name)]})
        rows.append({'reason': 'build-finished', 'success': True})
        return rows

    def cargo_bytes(self, rows):
        return b'\n'.join(json.dumps(row).encode() for row in rows)

    def test_effective_profiles_reject_debug_overrides_and_missing_components(self):
        root = Path('/build').resolve()
        for mutation in ('unoptimized-ffi', 'unoptimized-vm', 'debug', 'test', 'missing', 'host-only', 'duplicate', 'unfinished'):
            rows = self.cargo_messages(root)
            ffi = next(row for row in rows if row.get('target', {}).get('name') == 'lithovm_ffi')
            if mutation.startswith('unoptimized'):
                victim = ffi if mutation == 'unoptimized-ffi' else next(r for r in rows if r.get('target', {}).get('name') == 'lithovm')
                victim['profile']['opt_level'] = '0'
            elif mutation in ('debug', 'test'):
                ffi['profile']['debug_assertions' if mutation == 'debug' else 'test'] = True
            elif mutation == 'missing':
                rows.remove(ffi)
            elif mutation == 'host-only':
                ffi['filenames'] = [str(root / 'target/release/liblithovm_ffi.so')]
            elif mutation == 'duplicate':
                rows.insert(0, ffi)
            else:
                rows.pop()
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, 'Cargo|release profile'):
                release.cargo_profiles(self.cargo_bytes(rows), root, 'x86_64-unknown-linux-gnu')

    def test_profiles_cover_target_dependencies_and_ignore_host_build_tools(self):
        root = Path('/build').resolve()
        rows = self.cargo_messages(root)
        dependency = {'reason': 'compiler-artifact', 'package_id': 'registry+https://example.invalid#dependency@1.0',
                      'manifest_path': str(root.parent / 'registry/dependency/Cargo.toml'),
                      'target': {'name': 'dependency', 'kind': ['lib']},
                      'profile': dict(rows[0]['profile']),
                      'filenames': [str(root / 'target/x86_64-unknown-linux-gnu/release/deps/libdependency.rlib')]}
        rows.insert(0, dependency)
        host = dict(dependency, target={'name': 'build_script_build', 'kind': ['custom-build']},
                    profile=dict(dependency['profile'], opt_level='0', debug_assertions=True),
                    filenames=[str(root / 'target/release/build/dependency/build-script-build')])
        rows.insert(0, host)
        profiles = release.cargo_profiles(self.cargo_bytes(rows), root, 'x86_64-unknown-linux-gnu')
        self.assertEqual(len(profiles), len(release.RELEASE_COMPONENTS) + 1)
        self.assertNotIn(str(root), json.dumps(profiles))
        dependency['profile']['opt_level'] = '0'
        with self.assertRaisesRegex(ValueError, 'release profile'):
            release.cargo_profiles(self.cargo_bytes(rows), root, 'x86_64-unknown-linux-gnu')

    def test_build_command_checks_effective_profiles_before_payloads(self):
        root = Path('/build').resolve()
        rows = self.cargo_messages(root)
        rows[0]['profile']['opt_level'] = '0'
        with mock.patch.object(release, 'run', return_value=self.cargo_bytes(rows)) as command:
            with self.assertRaisesRegex(ValueError, 'release profile'):
                release.build_release_profiles(['cargo', '+1.96.0', 'build', '--release'], root, {},
                                               'x86_64-unknown-linux-gnu')
        self.assertIn('--message-format=json', command.call_args.args[0])

    def test_cargo_canonical_paths_match_aliased_temporary_build_root(self):
        root = Path('/build').resolve()
        messages = self.cargo_bytes(self.cargo_messages(root))
        self.assertEqual(release.cargo_profiles(messages, root / 'alias' / '..', 'x86_64-unknown-linux-gnu'),
                         release.cargo_profiles(messages, root, 'x86_64-unknown-linux-gnu'))

    def test_new_archive_policy_requires_matching_profiles_and_macos_relink(self):
        root = Path('/build').resolve()
        for target in ('x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc', 'aarch64-apple-darwin'):
            profiles = release.cargo_profiles(self.cargo_bytes(self.cargo_messages(root, target)), root, target)
            stages = {'build': profiles}
            if 'apple' in target:
                stages['native_library_relink'] = [p for p in profiles if p['target'] == 'lithovm_ffi']
            evidence = {'build_profile_policy': release.BUILD_PROFILE_POLICY, 'cargo_profiles': [stages, stages]}
            release.verify_build_profiles(evidence, target)
            for mutation in ('none', 'one-build', 'mismatch', 'unoptimized', 'unknown-policy', 'missing-stage', 'valid'):
                files = self.files(target, release.CUSTODY_SMOKE_PROFILE)
                current = json.loads(json.dumps(evidence))
                if mutation == 'none':
                    current.pop('cargo_profiles')
                elif mutation == 'one-build':
                    current['cargo_profiles'].pop()
                elif mutation == 'mismatch':
                    current['cargo_profiles'][1]['build'][0]['profile']['overflow_checks'] = True
                elif mutation == 'unoptimized':
                    for build in current['cargo_profiles']:
                        build['build'][0]['profile']['opt_level'] = '0'
                elif mutation == 'unknown-policy':
                    current['build_profile_policy'] = 'unknown'
                elif mutation == 'missing-stage':
                    for build in current['cargo_profiles']:
                        build.pop('native_library_relink' if 'apple' in target else 'build')
                files['build-evidence.json'] = release.canonical(current)
                manifest = json.loads(files['manifest.json'])
                manifest['build_profile_policy'] = release.BUILD_PROFILE_POLICY
                manifest['files']['build-evidence.json'] = {
                    'sha256': release.sha(files['build-evidence.json']), 'bytes': len(files['build-evidence.json'])}
                files['manifest.json'] = release.canonical(manifest)
                raw = release.archive_bytes(files, 1700000000, windows='windows' in target)
                with self.subTest(target=target, mutation=mutation):
                    if mutation == 'valid':
                        release.verify_archive(raw)
                    else:
                        with self.assertRaisesRegex(ValueError, 'Cargo release profile'):
                            release.verify_archive(raw)

    def source_repo(self, directory):
        root = Path(directory).resolve() / 'source'
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
            clone = Path(directory).resolve() / 'transformed-checkout'
            subprocess.run(['git', 'clone', '-c', 'core.autocrlf=true', str(root), str(clone)],
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
            clone = Path(directory).resolve() / 'windows-checkout'
            subprocess.run(['git', 'clone', '-c', 'core.autocrlf=true', str(root), str(clone)],
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

    def files(self, target, profile=release.LEGACY_SMOKE_PROFILE):
        names = set(release.artifact_names(target).values()) | {
            'include/lithovm.h', 'sbom.cdx.json', 'Cargo.lock', 'source-manifest.json',
            'build-evidence.json', 'smoke-evidence.json'}
        files = {n: n.encode() for n in names}
        if profile in (release.ACCOUNT_SMOKE_PROFILE, release.CUSTODY_SMOKE_PROFILE):
            for name in (release.ACCOUNT_SOURCE, release.SIGNATURE_SOURCE, release.SIGNATURE_VECTOR):
                files[name] = name.encode()
            if profile == release.CUSTODY_SMOKE_PROFILE:
                files[release.CUSTODY_SOURCE] = release.CUSTODY_SMOKE
        files['manifest.json'] = release.canonical({'schema_version': 1, 'version': 'lithic-vtest',
            'target': target, 'smoke_profile': profile,
            'files': {n: {'sha256': release.sha(raw), 'bytes': len(raw)}
                                       for n, raw in files.items()}})
        return files

    def test_account_profile_requires_its_sources_and_pinned_vector(self):
        for name in (release.ACCOUNT_SOURCE, release.SIGNATURE_SOURCE, release.SIGNATURE_VECTOR):
            files = self.files('x86_64-unknown-linux-gnu', release.ACCOUNT_SMOKE_PROFILE)
            del files[name]
            manifest = json.loads(files['manifest.json'])
            del manifest['files'][name]
            files['manifest.json'] = release.canonical(manifest)
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'required'):
                release.verify_archive(release.archive_bytes(files, 1700000000))

    def test_custody_profile_requires_its_actual_smoke_source(self):
        files = self.files('x86_64-unknown-linux-gnu', release.CUSTODY_SMOKE_PROFILE)
        del files[release.CUSTODY_SOURCE]
        manifest = json.loads(files['manifest.json'])
        del manifest['files'][release.CUSTODY_SOURCE]
        files['manifest.json'] = release.canonical(manifest)
        with self.assertRaisesRegex(ValueError, 'required'):
            release.verify_archive(release.archive_bytes(files, 1700000000))

    def test_smoke_profiles_are_explicit_and_old_archives_still_verify(self):
        files = self.files('x86_64-unknown-linux-gnu')
        manifest = json.loads(files['manifest.json'])
        del manifest['smoke_profile']
        files['manifest.json'] = release.canonical(manifest)
        release.verify_archive(release.archive_bytes(files, 1700000000))
        manifest['smoke_profile'] = 'unknown-profile'
        files['manifest.json'] = release.canonical(manifest)
        with self.assertRaisesRegex(ValueError, 'smoke profile'):
            release.verify_archive(release.archive_bytes(files, 1700000000))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'manifest.json').write_bytes(release.canonical({
                'smoke_profile': release.ACCOUNT_SMOKE_PROFILE}))
            with self.assertRaisesRegex(ValueError, 'contradicts'):
                release.smoke(root, release.artifact_names('x86_64-unknown-linux-gnu'),
                              release.LEGACY_SMOKE_PROFILE)

    def test_account_smoke_retains_vector_pin_without_private_key_fields(self):
        source = {name: (release.ROOT / name).read_bytes()
                  for name in (release.ACCOUNT_SOURCE, release.VECTORS_SOURCE)}
        files = release.account_smoke_inputs(source)
        self.assertEqual(files[release.ACCOUNT_SOURCE], source[release.ACCOUNT_SOURCE])
        vector = json.loads(files[release.SIGNATURE_VECTOR])
        self.assertEqual(set(vector), {'digest', 'signature', 'address', 'source_vectors_sha256'})
        self.assertEqual(vector['source_vectors_sha256'], release.VECTORS_SHA256)
        self.assertEqual(len(bytes.fromhex(vector['signature'][2:])), 65)
        source[release.VECTORS_SOURCE] += b'\n'
        with self.assertRaisesRegex(ValueError, 'pin changed'):
            release.account_smoke_inputs(source)

    def test_archive_deterministic_and_complete_for_all_release_targets(self):
        for target in ('x86_64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-pc-windows-msvc'):
            for profile in (release.LEGACY_SMOKE_PROFILE, release.ACCOUNT_SMOKE_PROFILE, release.CUSTODY_SMOKE_PROFILE):
                with self.subTest(target=target, profile=profile):
                    files = self.files(target, profile)
                    first = release.archive_bytes(files, 1700000000, windows='windows' in target)
                    second = release.archive_bytes(dict(reversed(list(files.items()))), 1700000000,
                                                   windows='windows' in target)
                    self.assertEqual(first, second)
                    manifest = release.verify_archive(first)
                    self.assertEqual(manifest['target'], target)
                    self.assertEqual(manifest['smoke_profile'], profile)

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
