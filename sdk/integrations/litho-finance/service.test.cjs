const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const ts = require('typescript');

const factory = '0x1111111111111111111111111111111111111111';
const token = '0x2222222222222222222222222222222222222222';
const other = '0x3333333333333333333333333333333333333333';
const hash = `0x${'ab'.repeat(32)}`;
const source = fs.readFileSync(path.join(__dirname, 'frontend/TokenCreationService.ts'), 'utf8');
const compiled = ts.transpileModule(source, { compilerOptions: {
  module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022,
} }).outputText;

function setup({ status = 'success', logs, configured = true } = {}) {
  const calls = { submitted: [], writes: [], units: [], availability: 0 };
  const event = (address = factory, args = {}) => ({ address, topics: [], data: {
    eventName: 'TokenCreated', args: { token, name: 'Test', symbol: 'TT', ...args },
  } });
  const modules = {
    viem: {
      decodeEventLog: ({ data }) => { if (data === 'malformed') throw Error('decode'); return data; },
      parseUnits: (...args) => { calls.units.push(args); return 1000000n; },
    },
    'wagmi/actions': {
      writeContract: async (_, request) => { calls.writes.push(request); return hash; },
      waitForTransactionReceipt: async () => ({ status, logs: logs ?? [event()] }),
    },
    './base': {
      assertServiceAvailable: () => { calls.availability++; },
      ContractNotDeployedError: class extends Error {},
    },
    '@/config/chains': { getChainById: () => ({ name: 'Makalu', contracts: { tokenFactory: configured ? factory : undefined } }) },
    '@/providers/wagmiConfig': { wagmiConfig: {} },
    '@/config/abis/lithoTokenFactory': { LITHO_TOKEN_FACTORY_ABI: [] },
  };
  const exports = {};
  new Function('require', 'exports', compiled)((id) => {
    if (!(id in modules)) throw Error(`Unexpected dependency ${id}`);
    return modules[id];
  }, exports);
  const params = { chainId: '700777', name: 'Test', symbol: 'TT', totalSupply: '1', decimals: 6,
    features: { mintable: true, burnable: false, pausable: true, ownership: false }, contractLanguage: 'solidity' };
  const ctx = { onSubmitted: (value) => calls.submitted.push(value) };
  return { calls, params, ctx, event, deploy: exports.TokenCreationService.deployToken };
}

test('Lithic rejects without availability checks, wallet calls or submitted callbacks', async () => {
  const s = setup();
  await assert.rejects(s.deploy({ ...s.params, contractLanguage: 'lithic' }, s.ctx), /not available/);
  assert.deepEqual(s.calls, { submitted: [], writes: [], units: [], availability: 0 });
});

test('Solidity preserves supply, flags, chain and real submission while returning emitted address', async () => {
  const s = setup();
  assert.deepEqual(await s.deploy(s.params, s.ctx), { hash, contractAddress: token });
  assert.deepEqual(s.calls.submitted, [hash]);
  assert.deepEqual(s.calls.units, [['1', 6]]);
  assert.equal(s.calls.writes[0].chainId, 700777);
  assert.equal(s.calls.writes[0].address, factory);
  assert.deepEqual(s.calls.writes[0].args, ['Test', 'TT', 6, 1000000n, true, false, true, false]);
});

test('missing factory does not submit a transaction', async () => {
  const s = setup({ configured: false });
  await assert.rejects(s.deploy(s.params, s.ctx), /No token factory/);
  assert.equal(s.calls.writes.length, 0);
});

test('reverted receipt is not reported as token creation success', async () => {
  const s = setup({ status: 'reverted' });
  await assert.rejects(s.deploy(s.params, s.ctx), /reverted/);
  assert.deepEqual(s.calls.submitted, [hash]);
});

test('rejects missing, malformed, foreign, duplicate, mismatched and zero-address creation events', async () => {
  const { event } = setup();
  for (const logs of [[], [{ address: factory, topics: [], data: 'malformed' }], [event(other)],
    [event(), event()], [event(factory, { name: 'Wrong' })],
    [event(factory, { symbol: 'WRONG' })], [event(factory, { token: `0x${'0'.repeat(40)}` })]]) {
    const s = setup({ logs });
    await assert.rejects(s.deploy(s.params, s.ctx));
  }
});

test('ignores foreign and unrelated logs when exactly one authentic creation event exists', async () => {
  const { event } = setup();
  const s = setup({ logs: [event(other), { address: factory, topics: [], data: 'malformed' }, event()] });
  assert.deepEqual(await s.deploy(s.params, s.ctx), { hash, contractAddress: token });
});
