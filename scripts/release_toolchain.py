"""Build and verify a reproducible compiler/native-runtime release candidate.

Python 3.11+, the pinned Rust toolchain, git and the native linker are required.
This command never publishes a release or contacts a blockchain.
"""
import argparse
import ctypes
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import tomllib
import zipfile

TOOLS = ('lithc', 'lithverify', 'lithdev', 'lithfmt', 'lithlint')
ROOT = Path(__file__).resolve().parents[1]
SMOKE = b'''contract ReleaseSmoke {
    pub fn answer() -> u64 { return 42; }
}
'''
DOCS = ('README.md', 'LICENSE', 'docs/IMPLEMENTATION_STATUS.md',
        'docs/RELEASE_PROCESS.md', 'docs/LEP100_15_IMPLEMENTATION_DECISIONS.md',
        'docs/LTH_R3_RATIFICATION_2026_10_01.md', 'docs/PRODUCTION_COMPLETION_2026_10_25.md',
        'docs/LITHDEV_NATIVE_PREPARATION.md')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def canonical(value):
    return (json.dumps(value, sort_keys=True, indent=2) + '\n').encode()


def run(argv, *, cwd=ROOT, env=None):
    result = subprocess.run(argv, cwd=cwd, env=env, capture_output=True)
    if result.returncode:
        raise RuntimeError(f'command failed ({result.returncode}): {argv}\n' +
                           result.stderr.decode(errors='replace'))
    return result.stdout


def safe_name(name):
    path = PurePosixPath(name)
    if not name or '\\' in name or ':' in name or path.is_absolute() or \
            any(p in ('', '.', '..') for p in name.split('/')):
        raise ValueError('noncanonical release path: ' + name)
    return name


def artifact_names(target):
    if target == 'x86_64-unknown-linux-gnu':
        return {**{n: 'bin/' + n for n in TOOLS}, 'liblithovm_ffi.so': 'lib/liblithovm_ffi.so'}
    if target == 'aarch64-apple-darwin':
        return {**{n: 'bin/' + n for n in TOOLS},
                'liblithovm_ffi.dylib': 'lib/liblithovm_ffi.dylib'}
    if target == 'x86_64-pc-windows-msvc':
        return {**{n + '.exe': 'bin/' + n + '.exe' for n in TOOLS},
                'lithovm_ffi.dll': 'lib/lithovm_ffi.dll'}
    raise ValueError('unsupported release target')


def source_snapshot(root):
    listing = run(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'], cwd=root)
    files = {}
    for encoded in listing.split(b'\0'):
        if not encoded:
            continue
        name = safe_name(encoded.decode('utf-8'))
        path = root / name
        if path.is_symlink() or any(p.is_symlink() for p in path.parents):
            raise ValueError('symlink source input: ' + name)
        if not path.is_file():
            raise ValueError('missing/nonregular source input: ' + name)
        files[name] = path.read_bytes()
    if not {'Cargo.lock', 'Cargo.toml', 'rust-toolchain.toml'} <= set(files):
        raise ValueError('incomplete source snapshot')
    return files


def sbom(metadata, lock):
    checksums = {(p['name'], p['version'], p.get('source')): p.get('checksum')
                 for p in lock['package']}
    refs, components = {}, []
    for package in metadata['packages']:
        name, version, source = package['name'], package['version'], package['source']
        ref = name + '@' + version + '#' + (source or 'workspace')
        if ref in refs.values():
            raise ValueError('ambiguous dependency identity')
        refs[package['id']] = ref
        properties = [{'name': 'cargo:source', 'value': source or 'workspace'},
                      {'name': 'cargo:declared-license', 'value': package.get('license') or 'UNSPECIFIED'}]
        item = {'type': 'library', 'bom-ref': ref, 'name': name, 'version': version,
                'purl': f'pkg:cargo/{name}@{version}', 'properties': properties}
        checksum = checksums.get((name, version, source))
        if checksum:
            item['hashes'] = [{'alg': 'SHA-256', 'content': checksum}]
        components.append(item)
    dependencies = [{'ref': refs[node['id']],
                     'dependsOn': sorted({refs[d] for d in node['dependencies']})}
                    for node in metadata['resolve']['nodes']]
    return {'bomFormat': 'CycloneDX', 'specVersion': '1.5', 'version': 1,
            'metadata': {'properties': [{'name': 'scope', 'value':
                'Cargo.lock resolved workspace graph, including development/optional dependencies; not a vulnerability scan'}]},
            'components': sorted(components, key=lambda p: p['bom-ref']),
            'dependencies': sorted(dependencies, key=lambda p: p['ref'])}


def archive_bytes(files, epoch, windows=False):
    if any(safe_name(name) != name for name in files):
        raise ValueError('invalid archive name')
    buffer = io.BytesIO()
    if windows:
        import datetime
        stamp = datetime.datetime.fromtimestamp(max(epoch, 315532800), datetime.timezone.utc)
        with zipfile.ZipFile(buffer, 'w') as archive:
            for name, raw in sorted(files.items()):
                info = zipfile.ZipInfo(name, stamp.timetuple()[:6])
                info.create_system = 3
                info.external_attr = (0o100755 if name.startswith('bin/') else 0o100644) << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, raw)
    else:
        with gzip.GzipFile(filename='', mode='wb', fileobj=buffer, mtime=epoch) as compressed:
            with tarfile.open(fileobj=compressed, mode='w') as archive:
                for name, raw in sorted(files.items()):
                    info = tarfile.TarInfo(name)
                    info.size, info.mtime = len(raw), epoch
                    info.mode = 0o755 if name.startswith('bin/') else 0o644
                    archive.addfile(info, io.BytesIO(raw))
    return buffer.getvalue()


def preserve_rebuild_failure(builds, roots, output, target, commit):
    """Retain actual failing payloads and metadata instead of deleting the repro."""
    destination = output.with_name(output.name + '-failed-rebuild')
    destination.mkdir(parents=True)
    differences = {}
    for name in builds[0]:
        left, right = builds[0][name], builds[1][name]
        if left == right:
            continue
        first = next((i for i, (a, b) in enumerate(zip(left, right)) if a != b), min(len(left), len(right)))
        row = {'first_difference': first, 'lengths': [len(left), len(right)],
               'sha256': [sha(left), sha(right)], 'metadata': []}
        for index, raw in enumerate((left, right)):
            path = destination / ('build-' + str(index)) / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
            metadata = {'embedded_build_root': any(pattern in raw for pattern in (
                str(roots[index]).encode(), str(roots[index]).replace('\\', '/').encode(),
                str(roots[index]).encode('utf-16-le')))}
            if raw[:2] == b'MZ' and len(raw) >= 64:
                pe = int.from_bytes(raw[60:64], 'little')
                if raw[pe:pe + 4] == b'PE\0\0':
                    metadata['coff_timestamp'] = int.from_bytes(raw[pe + 8:pe + 12], 'little')
                pos = raw.find(b'RSDS')
                if pos >= 0:
                    metadata['codeview_guid_age'] = raw[pos + 4:pos + 24].hex()
                    metadata['pdb_path'] = raw[pos + 24:].split(b'\0', 1)[0].decode(errors='replace')
            if target.endswith('apple-darwin'):
                try:
                    inspection = subprocess.run(['/usr/bin/otool', '-l', str(path)], capture_output=True, timeout=15)
                    (path.with_name(path.name + '.otool.txt')).write_bytes(inspection.stdout + inspection.stderr)
                    metadata['otool_exit'] = inspection.returncode
                except (OSError, subprocess.TimeoutExpired) as error:
                    metadata['otool_error'] = type(error).__name__
            row['metadata'].append(metadata)
        differences[name] = row
    (destination / 'FAILED_REBUILD.json').write_bytes(canonical({
        'status': 'FAILED_REBUILD_NOT_A_RELEASE', 'source_commit': commit,
        'target': target, 'differences': differences}))


def verify_archive(raw):
    files = {}
    if raw.startswith(b'PK'):
        with zipfile.ZipFile(io.BytesIO(raw)) as archive:
            for entry in archive.infolist():
                name = safe_name(entry.filename)
                if name in files or entry.is_dir() or (entry.external_attr >> 16) & 0o170000 != 0o100000:
                    raise ValueError('duplicate/nonregular archive entry')
                files[name] = archive.read(entry)
    else:
        with tarfile.open(fileobj=io.BytesIO(raw), mode='r:gz') as archive:
            for entry in archive:
                name = safe_name(entry.name)
                if name in files or not entry.isfile():
                    raise ValueError('duplicate/nonregular archive entry')
                files[name] = archive.extractfile(entry).read()
    manifest = json.loads(files['manifest.json'])
    if set(files) != set(manifest['files']) | {'manifest.json'}:
        raise ValueError('release archive inventory mismatch')
    for name, pin in manifest['files'].items():
        if len(files[name]) != pin['bytes'] or sha(files[name]) != pin['sha256']:
            raise ValueError('release hash/length mismatch: ' + name)
    required = set(artifact_names(manifest['target']).values()) | {
        'include/lithovm.h', 'sbom.cdx.json', 'Cargo.lock', 'source-manifest.json',
        'build-evidence.json', 'smoke-evidence.json'}
    if not required <= set(files):
        raise ValueError('release is missing a required compiler/runtime artifact')
    return manifest


def smoke(package_dir, names):
    compiler = package_dir / names['lithc.exe' if os.name == 'nt' else 'lithc']
    source = package_dir / 'release-smoke.lithic'
    artifact = json.loads(run([str(compiler), '--emit', 'lithovm', str(source)], cwd=package_dir))
    dev = package_dir / names['lithdev.exe' if os.name == 'nt' else 'lithdev']
    prepared = json.loads(run([str(dev), 'prepare-deploy', str(source), '--chain-id', '700777'], cwd=package_dir))
    if prepared['artifact'] != artifact or prepared['executionAuthorized'] is not False:
        raise ValueError('packaged lithdev source/artifact preparation mismatch')
    call_intent = json.loads(run([str(dev), 'prepare-call', str(source), '--chain-id', '700777',
        '--contract', '0x' + '11' * 20, '--function', 'answer'], cwd=package_dir))
    if not call_intent['data'].startswith('0xcf511cf7') or call_intent['deploymentStatus'] != 'not-submitted':
        raise ValueError('packaged lithdev call preparation mismatch')
    bytecode = bytes.fromhex(artifact['bytecode'][2:])
    library_name = next(k for k in names if 'ffi' in k)
    library = ctypes.CDLL(str(package_dir / names[library_name]))
    class Buffer(ctypes.Structure):
        _fields_ = [('data', ctypes.POINTER(ctypes.c_ubyte)), ('len', ctypes.c_size_t)]
    read_type = ctypes.CFUNCTYPE(ctypes.c_ssize_t, ctypes.c_size_t, ctypes.c_void_p,
                               ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t)
    charge_type = ctypes.CFUNCTYPE(ctypes.c_int32, ctypes.c_size_t, ctypes.c_uint64)
    state, charges, callback_errors = {}, [], []
    @read_type
    def read(_, key, length, output, capacity):
        try:
            value = state.get(ctypes.string_at(key, length).decode())
            if value is None:
                return -1
            if capacity:
                if capacity < len(value):
                    return -2
                ctypes.memmove(output, value, len(value))
            return len(value)
        except Exception:
            callback_errors.append('read callback failed')
            return -2
    @charge_type
    def charge(_, amount):
        charges.append(amount)
        return 1
    execute = library.lithovm_execute_v2
    execute.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t, read_type,
                        charge_type, ctypes.POINTER(Buffer)]
    execute.restype = ctypes.c_int32
    library.lithovm_free_v1.argtypes = [Buffer]
    library.lithovm_free_v1.restype = None
    request = {'version': 1, 'operation': 'deploy', 'caller': '0x' + '11' * 20,
               'bytecode': artifact['bytecode'], 'arguments': '0x4c56414c010000',
               'nonce': 0, 'gas_limit': 10_000_000, 'chain_id': 700780,
               'block_height': 1, 'block_timestamp': 1}
    def invoke(value):
        raw = canonical(value)
        output = Buffer()
        rc = execute(raw, len(raw), 0, read, charge, ctypes.byref(output))
        try:
            if rc or callback_errors:
                raise ValueError('packaged native FFI execution failed')
            return json.loads(ctypes.string_at(output.data, output.len))
        finally:
            library.lithovm_free_v1(output)
    deployed = invoke(request)
    if deployed.get('success') is not True or len(deployed['deployments']) != 1:
        raise ValueError('packaged compiler/native deploy failed')
    for write in deployed['writes']:
        if write.get('delete'):
            state.pop(write['key'], None)
        else:
            state[write['key']] = bytes.fromhex(write['value'][2:])
    address = deployed['deployments'][0]['contract']
    call = {k: v for k, v in request.items() if k != 'bytecode'}
    call.update(operation='call', contract=address, function='answer')
    called = invoke(call)
    expected = '0x' + (b'LVAL\x01\x00\x01\x01' + (42).to_bytes(32, 'big')).hex()
    if called.get('success') is not True or called['result'] != expected or not charges:
        raise ValueError('packaged compiler/runtime answer mismatch')
    returned = bytes.fromhex(called['result'][2:])
    abi_return = (32).to_bytes(32, 'big') + len(returned).to_bytes(32, 'big') + returned
    abi_return += bytes((-len(returned)) % 32)
    decoded = json.loads(run([str(dev), 'decode-result', '0x' + abi_return.hex()], cwd=package_dir))
    if decoded['values'] != [{'type': 'u64', 'value': '42'}] or decoded['canonicalProofVerified'] is not False:
        raise ValueError('packaged lithdev native-result decoding mismatch')
    failed = invoke({**call, 'function': 'missing'})
    if failed.get('success') is not False or failed['writes'] or failed['events']:
        raise ValueError('packaged runtime failure guard failed')
    # Exercise the actual packaged source verifier, including a tamper control.
    code_file = package_dir / 'smoke-code.bin'
    code_file.write_bytes(bytecode)
    verification = {'schemaVersion': 'lithovm-verification-v1', 'chainId': 700780,
                    'contractAddress': address, 'codeHash': artifact['codeHash'],
                    'artifact': artifact, 'sources': [{'path': 'release-smoke.lithic',
                        'content': SMOKE.decode(), 'keccak256': artifact['sourceHash']}]}
    verification_file = package_dir / 'smoke-verification.json'
    verification_file.write_bytes(canonical(verification))
    verifier = package_dir / names['lithverify.exe' if os.name == 'nt' else 'lithverify']
    verified = json.loads(run([str(verifier), str(verification_file), '700780', address,
                              str(code_file)], cwd=package_dir))
    code_file.write_bytes(bytecode[:-1] + bytes([bytecode[-1] ^ 1]))
    tampered = subprocess.run([str(verifier), str(verification_file), '700780', address,
                              str(code_file)], capture_output=True)
    if tampered.returncode == 0:
        raise ValueError('packaged verifier accepted modified bytecode')
    return {'compiler_runtime_deploy_call': 'PASS', 'answer': 42,
            'lithdev_unsigned_deploy_call_preparation': 'PASS',
            'lithdev_native_result_decode': 'PASS',
            'ffi_v2_fuel_callback': 'PASS', 'invalid_call_no_effects': 'PASS',
            'source_verifier': 'PASS', 'modified_code_rejected': 'PASS',
            'source_hash': artifact['sourceHash'], 'code_hash': verified['rebuiltCodeHash'],
            'scope': 'local packaged native library; no chain registration or deployment'}


def build(args):
    if not re.fullmatch(r'lithic-v[0-9A-Za-z][0-9A-Za-z._-]*', args.version):
        raise ValueError('invalid release version')
    output = Path(args.output).resolve()
    if output.exists() or output.is_symlink():
        raise ValueError('release output must not exist')
    if output.is_relative_to(ROOT):
        raise ValueError('release output must be outside source checkout')
    status = run(['git', 'status', '--porcelain=v1'], cwd=ROOT).decode()
    if status and not args.allow_dirty:
        raise ValueError('source checkout is dirty; local candidates require --allow-dirty')
    source = source_snapshot(ROOT)
    commit = run(['git', 'rev-parse', 'HEAD']).decode().strip()
    epoch = int(run(['git', 'show', '-s', '--format=%ct', 'HEAD']).strip())
    toolchain = tomllib.loads(source['rust-toolchain.toml'].decode())['toolchain']['channel']
    if not re.fullmatch(r'\d+\.\d+\.\d+', toolchain):
        raise ValueError('release requires an exact Rust toolchain version')
    rustc = run(['rustc', '+' + toolchain, '-vV']).decode()
    host = next(line.split(': ', 1)[1] for line in rustc.splitlines() if line.startswith('host: '))
    if args.target != host:
        raise ValueError('release smoke must run on its native target host')
    names = artifact_names(args.target)
    builds, roots = [], []
    with tempfile.TemporaryDirectory(prefix='lithic-release-') as directory:
        temporary = Path(directory)
        for index in range(2):
            root = temporary / ('independent-' + str(index))
            roots.append(root)
            root.mkdir()
            for name, raw in source.items():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(raw)
            env = {k: v for k, v in os.environ.items() if k not in (
                'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_DIR', 'RUSTC_WRAPPER',
                'RUSTC_WORKSPACE_WRAPPER', 'RUSTC', 'RUSTDOC')}
            env.update(SOURCE_DATE_EPOCH=str(epoch), CARGO_INCREMENTAL='0',
                       CARGO_ENCODED_RUSTFLAGS='--remap-path-prefix=' + str(root) + '=/lithic')
            flags = ['--offline'] if args.offline else []
            run(['cargo', '+' + toolchain, 'build', '--release', '--locked', *flags,
                 '--target', args.target, '--target-dir', str(root / 'target'),
                 '-p', 'lithc', '-p', 'lithfmt', '-p', 'lithlint', '-p', 'lithovm-ffi'], cwd=root, env=env)
            binaries = {dest: (root / 'target' / args.target / 'release' / name).read_bytes()
                        for name, dest in names.items()}
            builds.append(binaries)
            if index == 0:
                metadata = json.loads(run(['cargo', '+' + toolchain, 'metadata', '--locked',
                    '--format-version', '1', '--filter-platform', args.target, *flags], cwd=root, env=env))
        if builds[0] != builds[1]:
            preserve_rebuild_failure(builds, roots, output, args.target, commit)
            changed = [name for name in builds[0] if builds[0][name] != builds[1][name]]
            raise ValueError('independent rebuild byte mismatch: ' + ', '.join(changed))
        package = temporary / 'package'
        package.mkdir()
        for name, raw in builds[0].items():
            path = package / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(raw)
            if name.startswith('bin/'):
                path.chmod(0o755)
        (package / 'release-smoke.lithic').write_bytes(SMOKE)
        # A separate process releases the loaded DLL before Windows cleanup.
        smoke_result = json.loads(run([sys.executable, str(Path(__file__).resolve()),
            'smoke', '--directory', str(package), '--target', args.target], cwd=package))
    # Never substitute a snapshot if the user edited sources while builds ran.
    if source_snapshot(ROOT) != source:
        raise ValueError('source changed during build; candidate not emitted')
    source_manifest = {n: {'bytes': len(raw), 'sha256': sha(raw)} for n, raw in sorted(source.items())}
    files = {**builds[0], 'release-smoke.lithic': SMOKE,
             'include/lithovm.h': source['packages/vm/lithovm-ffi/include/lithovm.h'],
             'Cargo.lock': source['Cargo.lock'], 'source-manifest.json': canonical(source_manifest),
             'sbom.cdx.json': canonical(sbom(metadata, tomllib.loads(source['Cargo.lock'].decode()))),
             'smoke-evidence.json': canonical(smoke_result),
             'build-evidence.json': canonical({'source_commit': commit, 'source_dirty': bool(status),
                 'source_inventory_sha256': sha(canonical(source_manifest)), 'source_date_epoch': epoch,
                 'rustc': rustc, 'target': args.target, 'independent_builds': 2,
                 'binary_hashes_equal': True, 'binary_sha256': {n: sha(raw) for n, raw in builds[0].items()},
                 'reproducibility_scope': 'same toolchain/host dependencies; distinct source and build roots',
                 'signed': False, 'production_approved': False})}
    for name in DOCS:
        if name in source:
            files['docs/' + PurePosixPath(name).name] = source[name]
    files['manifest.json'] = canonical({'schema_version': 1, 'version': args.version,
        'target': args.target, 'files': {n: {'bytes': len(raw), 'sha256': sha(raw)}
                                      for n, raw in sorted(files.items())}})
    raw = archive_bytes(files, epoch, windows='windows' in args.target)
    verify_archive(raw)
    suffix = '.zip' if 'windows' in args.target else '.tar.gz'
    name = args.version + '-' + args.target + suffix
    output.mkdir(parents=True)
    (output / name).write_bytes(raw)
    (output / (name + '.sha256')).write_text(sha(raw) + '  ' + name + '\n', encoding='ascii')
    print(json.dumps({'archive': str(output / name), 'sha256': sha(raw),
                      'rebuild_comparison': 'PASS', 'smoke': smoke_result}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    command = sub.add_parser('build')
    command.add_argument('--version', required=True)
    command.add_argument('--target', required=True)
    command.add_argument('--output', required=True)
    command.add_argument('--allow-dirty', action='store_true')
    command.add_argument('--offline', action='store_true')
    command = sub.add_parser('verify')
    command.add_argument('archive')
    command = sub.add_parser('smoke')
    command.add_argument('--directory', required=True)
    command.add_argument('--target', required=True)
    args = parser.parse_args()
    if args.command == 'build':
        build(args)
    elif args.command == 'verify':
        print(json.dumps(verify_archive(Path(args.archive).read_bytes()), indent=2))
    else:
        print(json.dumps(smoke(Path(args.directory).resolve(), artifact_names(args.target))))


if __name__ == '__main__':
    main()
