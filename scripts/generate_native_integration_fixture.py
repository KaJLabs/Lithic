#!/usr/bin/env python3
"""Local compiler/FFI fixture, not on-chain evidence. No RPC, wallet or signer."""
import argparse
import ctypes
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path


def canonical(value):
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode()


def generate(binary_dir):
    root = Path(__file__).resolve().parents[1]
    standards = root / 'sdk/contracts/standards'
    caller = '0x' + '33' * 20
    gateway = '0x' + '00' * 18 + 'f000'
    state, charges, errors = {}, [], []
    library = ctypes.CDLL(str(binary_dir / 'liblithovm_ffi.so'))

    class Buffer(ctypes.Structure):
        _fields_ = [('data', ctypes.POINTER(ctypes.c_ubyte)), ('len', ctypes.c_size_t)]

    read_type = ctypes.CFUNCTYPE(ctypes.c_ssize_t, ctypes.c_size_t, ctypes.c_void_p,
                                ctypes.c_size_t, ctypes.c_void_p, ctypes.c_size_t)
    charge_type = ctypes.CFUNCTYPE(ctypes.c_int32, ctypes.c_size_t, ctypes.c_uint64)

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
        except Exception as error:
            errors.append(str(error))
            return -2

    @charge_type
    def charge(_, amount):
        charges.append(amount)
        return 1

    execute = library.lithovm_execute_v2
    execute.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_size_t,
                        read_type, charge_type, ctypes.POINTER(Buffer)]
    execute.restype = ctypes.c_int32
    library.lithovm_free_v1.argtypes = [Buffer]

    def invoke(request):
        charges.clear()
        raw, output = canonical(request), Buffer()
        rc = execute(raw, len(raw), 0, read, charge, ctypes.byref(output))
        try:
            assert rc == 0 and not errors
            assert output.data and output.len <= 2 * 1024 * 1024
            result = json.loads(ctypes.string_at(output.data, output.len))
        finally:
            library.lithovm_free_v1(output)
        assert result['success'], result
        for write in result['writes']:
            if write.get('delete'):
                state.pop(write['key'], None)
            else:
                state[write['key']] = bytes.fromhex(write['value'][2:])
        return result

    def command(*args):
        return json.loads(subprocess.check_output([str(binary_dir / args[0]), *args[1:]]))

    def argument(kind, value):
        return {'type': kind, 'value': value}

    def native_arguments(intent):
        abi = bytes.fromhex(intent['data'][10:])
        offset = int.from_bytes(abi[64:96], 'big')
        size = int.from_bytes(abi[offset:offset + 32], 'big')
        return '0x' + abi[offset + 32:offset + 32 + size].hex()

    with tempfile.TemporaryDirectory(prefix='lithic-fixture-') as directory:
        work = Path(directory)

        def prepare(source, operation, arguments, *options):
            args_file = work / 'args.json'
            args_file.write_bytes(canonical(arguments))
            return command('lithdev', operation, str(source), '--chain-id', '700777',
                           '--args', str(args_file), *options)

        def deploy(intent, nonce):
            return invoke({'version': 1, 'operation': 'deploy', 'caller': caller,
                           'bytecode': intent['artifact']['bytecode'],
                           'function': 'initialize', 'arguments': native_arguments(intent),
                           'nonce': nonce, 'gas_limit': 10_000_000, 'chain_id': 700777,
                           'block_height': 42, 'block_timestamp': 1000})

        token_source = standards / 'finance_token_v12.lithic'
        factory_source = standards / 'finance_factory_v14.lithic'
        token = prepare(token_source, 'prepare-deploy', [
            argument('string', 'Template'), argument('string', 'TPL'),
            argument('address', caller), argument('u64', '6'), argument('u256', '0'),
            *[argument('bool', flag) for flag in [True, False, True, True]]
        ], '--initializer', 'initialize')
        template = deploy(token, 0)['deployments'][0]['contract']
        initializer = next(e['selector'] for e in token['artifact']['entrypoints']
                           if e['name'] == 'initialize')
        factory = prepare(factory_source, 'prepare-deploy', [
            argument('address', template), argument('bytes32', initializer)
        ], '--initializer', 'initialize')
        factory_address = deploy(factory, 1)['deployments'][0]['contract']
        salt = '0x' + '32' * 32
        args = [argument('bytes32', salt), argument('string', 'Client Δ Token'),
                argument('string', 'NAT'), argument('u64', '6'),
                argument('u256', '123450000'),
                *[argument('bool', flag) for flag in [True, False, True, True]]]
        intent = prepare(factory_source, 'prepare-call', args,
                         '--contract', factory_address, '--function', 'create')
        result = invoke({'version': 1, 'operation': 'call', 'caller': caller,
                         'contract': factory_address, 'function': 'create',
                         'arguments': native_arguments(intent), 'nonce': 2,
                         'gas_limit': 10_000_000, 'chain_id': 700777,
                         'block_height': 42, 'block_timestamp': 1000})
        child = result['deployments'][0]['contract']
        payload = {'Deployments': result['deployments'], 'Events': result['events']}
        receipt = {'schemaVersion': 'lithovm-evm-receipt-v1', 'chainId': '700777',
                   'transactionHash': '0x' + 'ab' * 32, 'from': caller, 'nonce': '2',
                   'blockHash': '0x' + 'cd' * 32, 'blockHeight': '42',
                   'transactionGasUsed': '9000000', 'status': 'success', 'logs': [{
                       'address': gateway, 'topics': [
                           '0x093843f5e9eb40384e28a2de05071eb8158d09a803dec22fa5b5fb21a61e29a2'],
                       'data': '0x' + canonical(payload).hex(), 'logIndex': '0'}]}
        receipt_file = work / 'receipt.json'
        receipt_file.write_bytes(canonical(receipt))
        observation = command('lithdev', 'decode-receipt', str(receipt_file),
                              '--chain-id', '700777', '--gateway', gateway)
        assert observation['deployments'][0]['contractAddress'] == child
        record = state['lithovm/v1/contracts/' + child[2:]]
        verification = {'schemaVersion': 'lithovm-verification-v1', 'chainId': 700777,
                        'contractAddress': child, 'codeHash': token['artifact']['codeHash'],
                        'artifact': token['artifact'], 'sources': [{
                            'path': token_source.name, 'content': token_source.read_text(),
                            'keccak256': token['artifact']['sourceHash']}]}
        verification_file, record_file = work / 'verify.json', work / 'record.bin'
        verification_file.write_bytes(canonical(verification))
        record_file.write_bytes(record)
        verified = command('lithverify', '--native-record', str(verification_file),
                           '700777', child, str(record_file))
        assert verified['rebuiltCodeHash'] == token['artifact']['codeHash']
        return {'scope': 'Synthetic local compiler/FFI fixture; no RPC, chain proof or deployment',
                'caller': caller, 'nonce': 2, 'gateway': gateway, 'receipt': receipt,
                'observation': observation, 'plan': {'intent': intent, 'expectation': {
                    'kind': 'child', 'factory': factory_address, 'template': template,
                    'codeHash': token['artifact']['codeHash'], 'userSalt': salt,
                    'token': {'name': 'Client Δ Token', 'symbol': 'NAT'}}},
                'verification': verification, 'verified': verified,
                'nativeRecord': '0x' + record.hex(), 'contractAddress': child,
                'binarySha256': {name: hashlib.sha256((binary_dir / name).read_bytes()).hexdigest()
                                 for name in ['lithc', 'lithdev', 'lithverify', 'liblithovm_ffi.so']}}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    options = parser.parse_args()
    options.output.write_bytes(canonical(generate(options.binary_dir)) + b'\n')
