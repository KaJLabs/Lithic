# Finance token metadata fixture

`finance_token_v12.lithic` extends the v11 behavior fixture with bounded UTF-8
name/symbol parameters and getters. It is a generic client-integration fixture,
not LAX, a contract factory, an ERC-20-compatible EVM deployment or a production
token. The v11 fixture and LAX candidate remain unchanged.

Initialize atomically using `TransactionalHost::deploy_values` with the function
`initialize(string,string,address,u64,u256,bool,bool,bool,bool)`:
name, symbol, creator, decimals, supply, mintable, burnable, pausable, ownership.
The supplied creator receives supply, independently of the deploying caller.
The owner is the creator only when ownership is enabled; otherwise it is zero.
The host initializer must be part of deployment, not a later public call.

The host tests exercise all 16 flag combinations, exact string getters,
decimals/supply, creator-versus-deployer balances, optional ownership and repeated
initialization rollback. The existing v11 tests still cover the underlying token
behavior. This is not exhaustive differential conformance against OpenZeppelin.

Remaining client gaps: contract-level factory creation, ordered synchronous
calls with arguments/return data, EVM interoperability where needed, persistent
chain/gateway/RPC integration, real frontend wallet/build tests, security review
and Makalu acceptance. No live deployment is supplied or authorized.
