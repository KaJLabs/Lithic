# Litho Finance service integration candidate

## Full frontend follow-up — 2026-09-26

Access to `amirmughal22/Litho-Finance` is now available using `bachal-mb`.
The safety changes were integrated locally from main `11078b5` into branch
`fix/lithic-deployment-safety`, commit `2822474`. The full app passed 24 tests
(18 new deployment tests using real viem ABI encoding/decoding), TypeScript,
lint and the production build. Native deployment remains disabled. No remote
push or live deployment was performed. Browser-wallet and on-chain end-to-end
checks remain outstanding; existing dependency install warnings also remain.
See that repository's `docs/LITHIC_INTEGRATION_STATUS.md` for exact scope.

## Original standalone package candidate

Based on Amir's six-file acceptance package; see
`../../../docs/LITHO_FINANCE_ACCEPTANCE_CASE.md` for its SHA-256 and scope.
`upstream/TokenCreationService.ts` preserves the supplied service;
`frontend/TokenCreationService.ts` is the replacement candidate for the
corresponding file in the real frontend. No live application was changed.

Changes:

- Native Lithic requests throw an explicit unavailable error before wallet
  submission, latency simulation, random identifiers or submission callbacks.
- Solidity calls retain their existing ABI, chain selection, supply conversion
  and feature flags. Receipts must succeed and contain exactly one matching
  TokenCreated event from the configured factory with the requested metadata
  and a valid nonzero token address.

Run `npm install` then `npm test` in this directory in a standalone checkout.
Tests transpile and execute the actual replacement service, mocking external
wallet/network/configuration dependencies. They do not establish compatibility
with the full application's TypeScript types, real ABI decoder, error display,
wallet UI or network. Full frontend build and application tests remain required.

Before merging into the frontend, verify that errors reach the UI without a
success notification and disable the native choice with a clear explanation.
Bind receipt creator to the submitting wallet once its authoritative account
is available from the application's action context. Do not guess that context's
shape from this excerpt. Token deployment and enabling native submission remain
outside this patch. Cost estimates in the original service remain illustrative.
