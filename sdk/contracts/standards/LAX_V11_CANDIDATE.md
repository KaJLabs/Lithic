# LAX Lithic v11 candidate

`lax_lep100_v11.lithic` is the first executable LEP100-oriented contract
fixture for the native compiler/runtime. It is not approved for deployment.

The compiled constants are:

| Property | Value |
| --- | --- |
| Name | `Lithosphere Algorithmic` (right-padded `bytes32`) |
| Symbol | `LAX` (right-padded `bytes32`) |
| Decimals | `18` |
| Initial/max issuance | `10,000,000,000 × 10^18` base units |

One successful `initialize(owner)` call assigns the complete initial issuance
to `owner`; later initialization reverts. There is no mint entrypoint. Burning
is supported and can reduce circulating and total supply, but supply can never
exceed the initial constant through the executable interface.

The host's deployment v1 conformance path now installs code and invokes this
initializer in one atomic state transaction. The eventual initial holder and transfers implementing the approved LITHO
allocation are intentionally absent. They require a reviewed deployment plan
and explicit deployment approval. Chain integration must preserve the tested
atomic deploy/init behavior so an unrelated caller cannot win the initializer.

The current native ABI has no string type, so name and symbol use padded
`bytes32` getters. Review must decide whether that is the production LEP100
metadata profile or whether a later ABI version adds strings. The review must
also approve zero-address behavior, burn policy, allowance race semantics,
event indexing, storage migration and the map gas schedule.

Compiler/runtime tests cover initialization, exact supply, balance transfer,
nested allowance lookup/update, delegated transfer, failed-balance rollback,
duplicate-initialization rollback and checked `u256` overflow rollback. Makalu,
RPC, explorer/source verification, long fuzz campaigns and independent audit
remain mandatory gates.
