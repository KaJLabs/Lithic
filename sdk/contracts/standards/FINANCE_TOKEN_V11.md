# Finance token behavior fixture

`finance_token_v11.lithic` exercises Amir's configurable token behavior through
the native compiler/runtime. It is not a complete factory port or production
token. It does not replace LAX or alter LAX supply/distribution policy.

Implemented: explicit creator initialization, configurable decimals (0..255)
and u256 supply, mint/burn/pause/ownership flags, ownership transfer and
renunciation, balances, finite/unlimited allowances, transfer/transfer_from,
burn/burn_from, and events. Pausing blocks transfers, minting and burning;
approvals remain possible. Initialization must be invoked atomically through
host deployment. Separate public initialization creates a first-caller race.

Tests run all 16 feature combinations, creator-versus-initializer-caller
identity, duplicate initialization, invalid decimals, unauthorized operations,
pause/unpause, ownership changes, allowance consumption, unlimited allowances,
and rollback of failed operations.

Not claimed: string name/symbol, Solidity ABI/selectors or custom-error parity,
factory child creation, exhaustive OpenZeppelin differential conformance,
native/EVM interoperability, consensus persistence, Makalu deployment or audit.

The compiler now permits optional else branches and continuing statements
after a conditional. Full function return coverage and branch-local scope are
still checked. v11 requires nonempty bytecode blocks, so an empty branch lowers
to `require(true)` with its normal gas charge. No existing bytecode version
changes; existing artifact verification vectors remain tested.
