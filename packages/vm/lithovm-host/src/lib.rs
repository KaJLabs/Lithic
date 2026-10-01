use lithovm::{
    EventRecord, ExecutionContext, ExecutionFailure, ExecutionOutcome, ExecutionResult,
    FailureKind, Storage, Vm,
};
use lithovm_bytecode::{function_selector, parse, values::Value, Program};
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;
pub mod deployment_status;
mod synchronous;

pub type Address = [u8; 32];
pub type Selector = [u8; 32];

/// Candidate limits/pricing, not an approved consensus schedule.
pub const MAX_CHILD_CODE_BYTES: usize = 65536;
pub const CHILD_CREATE_BASE_GAS: u64 = 100;

/// Bind a user salt to the immediate authenticated caller of the creating frame.
/// Routers are callers, not transparent proxies for transaction origin.
pub fn caller_bound_salt(caller: Address, user_salt: [u8; 32]) -> [u8; 32] {
    let mut hash = Keccak256::new();
    hash.update(b"LITHOVM_CALLER_SALT_V1");
    hash.update(caller);
    hash.update(user_salt);
    hash.finalize().into()
}

/// Low-level derivation: `salt` must be the effective caller-bound salt.
pub fn child_contract_address(
    creator: Address,
    salt: [u8; 32],
    code_hash: [u8; 32],
    chain_id: u64,
) -> Address {
    let mut hash = Keccak256::new();
    hash.update(b"LITHOVM_CREATE_V1");
    hash.update(chain_id.to_be_bytes());
    hash.update(creator);
    hash.update(salt);
    hash.update(code_hash);
    let digest = hash.finalize();
    let mut address = [0; 32];
    address[12..].copy_from_slice(&digest[12..]);
    address
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployedContract {
    pub bytecode: Vec<u8>,
    pub code_hash: [u8; 32],
    pub storage: Storage,
    pub entrypoints: BTreeMap<Selector, String>,
}

fn validated_entrypoints(
    program: &Program,
    address: Address,
) -> Result<BTreeMap<Selector, String>, HostFailure> {
    let mut entrypoints = BTreeMap::new();
    for function in &program.functions {
        if entrypoints
            .insert(function_selector(function), function.name.clone())
            .is_some()
        {
            return Err(HostFailure {
                kind: HostFailureKind::InvalidEntrypoint,
                message: "canonical function selector collision".into(),
                gas_used: 0,
                failed_contract: address,
            });
        }
    }
    Ok(entrypoints)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Initializer<V = [u8; 32]> {
    pub function: String,
    pub arguments: Vec<V>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeployRequest<V = [u8; 32]> {
    pub deployer: Address,
    pub nonce: u64,
    pub bytecode: Vec<u8>,
    pub initializer: Option<Initializer<V>>,
    pub value: [u8; 32],
    pub gas_limit: u64,
    pub block_height: u64,
    pub block_timestamp: u64,
    pub chain_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeploySuccess<V = [u8; 32]> {
    pub deployments: Vec<CommittedDeployment>,
    pub contract: Address,
    pub code_hash: [u8; 32],
    pub initializer_result: Option<ExecutionResult<V>>,
    pub gas_used: u64,
    pub events: Vec<CommittedEvent<V>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeployOutcome<V = [u8; 32]> {
    Success(DeploySuccess<V>),
    Failure(HostFailure),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallRequest<V = [u8; 32]> {
    pub contract: Address,
    pub function: String,
    pub arguments: Vec<V>,
    pub gas_limit: u64,
    pub caller: Address,
    pub value: [u8; 32],
    pub block_height: u64,
    pub block_timestamp: u64,
    pub chain_id: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedEvent<V = [u8; 32]> {
    pub contract: Address,
    pub event: EventRecord<V>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostSuccess<V = [u8; 32]> {
    pub deployments: Vec<CommittedDeployment>,
    pub result: ExecutionResult<V>,
    pub gas_used: u64,
    pub events: Vec<CommittedEvent<V>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostFailureKind {
    Vm(FailureKind),
    MissingContract,
    UnknownSelector,
    Reentrancy,
    InvalidBytecode,
    InvalidEntrypoint,
    ContractExists,
    InsufficientBalance,
    State,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostFailure {
    pub kind: HostFailureKind,
    pub message: String,
    pub gas_used: u64,
    pub failed_contract: Address,
}

/// Host-generated registration, published only after the enclosing commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommittedDeployment {
    pub creator: Address,
    pub contract: Address,
    pub code_hash: [u8; 32],
    pub origin: DeploymentOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeploymentOrigin {
    Transaction {
        nonce: u64,
    },
    /// `salt` is the effective caller-bound salt, never the raw user input.
    Child {
        template: Address,
        salt: [u8; 32],
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostOutcome<V = [u8; 32]> {
    Success(HostSuccess<V>),
    Failure(HostFailure),
}

/// A transaction-scoped view of chain state. Implementations must make
/// `commit` atomic and discard every write when dropped without committing.
pub trait StateTransaction {
    fn load_contract(&self, address: &Address) -> Result<Option<DeployedContract>, String>;
    fn store_contract(
        &mut self,
        address: Address,
        contract: DeployedContract,
    ) -> Result<(), String>;
    fn load_balance(&self, address: &Address) -> Result<[u8; 32], String>;
    fn store_balance(&mut self, address: Address, balance: [u8; 32]) -> Result<(), String>;
    fn commit(self) -> Result<(), String>;
}

/// Persistence seam implemented by the chain and by the conformance adapter.
pub trait TransactionalState {
    type Transaction<'a>: StateTransaction
    where
        Self: 'a;

    fn begin_transaction(&mut self) -> Result<Self::Transaction<'_>, String>;
}

pub struct TransactionalHost<S> {
    vm: Vm,
    state: S,
}

impl<S> TransactionalHost<S> {
    pub fn new(state: S) -> Self {
        Self {
            vm: Vm::default(),
            state,
        }
    }

    pub fn state(&self) -> &S {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    pub fn into_state(self) -> S {
        self.state
    }
}

impl<S: TransactionalState> TransactionalHost<S> {
    pub fn deploy(&mut self, request: DeployRequest) -> DeployOutcome {
        self.deploy_impl(request)
    }

    /// Local dynamic-value deployment candidate; not a broadcast API.
    pub fn deploy_values(&mut self, request: DeployRequest<Value>) -> DeployOutcome<Value> {
        self.deploy_impl(request)
    }

    fn deploy_impl<V: HostValue>(&mut self, request: DeployRequest<V>) -> DeployOutcome<V> {
        let code_hash = code_hash(&request.bytecode);
        let address =
            contract_address(request.deployer, request.nonce, code_hash, request.chain_id);
        let program = match parse(&request.bytecode) {
            Ok(program) => program,
            Err(error) => {
                return DeployOutcome::Failure(HostFailure {
                    kind: HostFailureKind::InvalidBytecode,
                    message: error.to_string(),
                    gas_used: 0,
                    failed_contract: address,
                })
            }
        };
        // Scalar entry points must not silently accept dynamic code.
        if !V::DYNAMIC && program.bytecode_version() >= lithovm_bytecode::STRING_VERSION {
            return DeployOutcome::Failure(HostFailure {
                kind: HostFailureKind::InvalidBytecode,
                message: "dynamic bytecode requires a dynamic-value host adapter".into(),
                gas_used: 0,
                failed_contract: address,
            });
        }
        let entrypoints = match validated_entrypoints(&program, address) {
            Ok(entrypoints) => entrypoints,
            Err(failure) => return DeployOutcome::Failure(failure),
        };
        if program
            .functions
            .iter()
            .any(|function| function.name == "initialize")
            && request
                .initializer
                .as_ref()
                .map(|init| init.function.as_str())
                != Some("initialize")
        {
            return DeployOutcome::Failure(HostFailure {
                kind: HostFailureKind::InvalidEntrypoint,
                message: "contract declaring initialize requires atomic initialization".into(),
                gas_used: 0,
                failed_contract: address,
            });
        }
        let mut transaction = match self.state.begin_transaction() {
            Ok(transaction) => transaction,
            Err(message) => return DeployOutcome::Failure(state_failure(message, 0, address)),
        };
        match transaction.load_contract(&address) {
            Ok(Some(_)) => {
                return DeployOutcome::Failure(HostFailure {
                    kind: HostFailureKind::ContractExists,
                    message: "derived contract address is already deployed".into(),
                    gas_used: 0,
                    failed_contract: address,
                })
            }
            Ok(None) => {}
            Err(message) => {
                return DeployOutcome::Failure(state_failure(message, 0, address));
            }
        }
        let deployer_balance = match load_balance(&transaction, request.deployer, 0, address) {
            Ok(balance) => balance,
            Err(failure) => return DeployOutcome::Failure(failure),
        };
        let Some(deployer_balance) = debit(deployer_balance, request.value) else {
            return DeployOutcome::Failure(HostFailure {
                kind: HostFailureKind::InsufficientBalance,
                message: "deployment value exceeds deployer balance".into(),
                gas_used: 0,
                failed_contract: address,
            });
        };
        if let Err(failure) = store_balance(
            &mut transaction,
            request.deployer,
            deployer_balance,
            0,
            address,
        ) {
            return DeployOutcome::Failure(failure);
        }
        let prior_balance = match load_balance(&transaction, address, 0, address) {
            Ok(balance) => balance,
            Err(failure) => return DeployOutcome::Failure(failure),
        };
        let Some(contract_balance) = credit(prior_balance, request.value) else {
            return DeployOutcome::Failure(state_failure(
                "deployment recipient balance overflow".into(),
                0,
                address,
            ));
        };
        if let Err(failure) = store_balance(&mut transaction, address, contract_balance, 0, address)
        {
            return DeployOutcome::Failure(failure);
        }
        if let Err(failure) = store_contract(
            &mut transaction,
            address,
            DeployedContract {
                bytecode: request.bytecode,
                code_hash,
                storage: Storage::default(),
                entrypoints,
            },
            0,
        ) {
            return DeployOutcome::Failure(failure);
        }

        let mut events = EventJournal {
            events: Vec::new(),
            value_bytes: 0,
            deployments: vec![CommittedDeployment {
                creator: request.deployer,
                contract: address,
                code_hash,
                origin: DeploymentOrigin::Transaction {
                    nonce: request.nonce,
                },
            }],
        };
        let mut active_contracts = vec![address];
        let initialized = if let Some(initializer) = request.initializer {
            let frame = FrameRequest {
                contract: address,
                function: initializer.function,
                arguments: initializer.arguments,
                gas_limit: request.gas_limit,
                caller: request.deployer,
                value: request.value,
                block_height: request.block_height,
                block_timestamp: request.block_timestamp,
                chain_id: request.chain_id,
                depth: 0,
            };
            match execute_frame(
                &self.vm,
                &mut transaction,
                frame,
                &mut events,
                &mut active_contracts,
            ) {
                Ok((result, gas_used)) => Some((result, gas_used)),
                Err(failure) => return DeployOutcome::Failure(failure),
            }
        } else {
            None
        };
        let (initializer_result, gas_used) = initialized
            .map(|(result, gas)| (Some(result), gas))
            .unwrap_or((None, 0));
        match transaction.commit() {
            Ok(()) => DeployOutcome::Success(DeploySuccess {
                deployments: events.deployments,
                contract: address,
                code_hash,
                initializer_result,
                gas_used,
                events: events.events,
            }),
            Err(message) => DeployOutcome::Failure(state_failure(message, gas_used, address)),
        }
    }

    pub fn execute(&mut self, request: CallRequest) -> HostOutcome {
        self.execute_impl(request)
    }

    /// Executes typed values against a transaction-scoped local state adapter.
    pub fn execute_values(&mut self, request: CallRequest<Value>) -> HostOutcome<Value> {
        self.execute_impl(request)
    }

    fn execute_impl<V: HostValue>(&mut self, request: CallRequest<V>) -> HostOutcome<V> {
        let mut transaction = match self.state.begin_transaction() {
            Ok(transaction) => transaction,
            Err(message) => {
                return HostOutcome::Failure(HostFailure {
                    kind: HostFailureKind::State,
                    message,
                    gas_used: 0,
                    failed_contract: request.contract,
                })
            }
        };
        let mut events = EventJournal {
            events: Vec::new(),
            value_bytes: 0,
            deployments: Vec::new(),
        };
        let mut active_contracts = vec![request.contract];
        let frame = FrameRequest {
            contract: request.contract,
            function: request.function,
            arguments: request.arguments,
            gas_limit: request.gas_limit,
            caller: request.caller,
            value: request.value,
            block_height: request.block_height,
            block_timestamp: request.block_timestamp,
            chain_id: request.chain_id,
            depth: 0,
        };
        match execute_frame(
            &self.vm,
            &mut transaction,
            frame,
            &mut events,
            &mut active_contracts,
        ) {
            Ok((result, gas_used)) => match transaction.commit() {
                Ok(()) => HostOutcome::Success(HostSuccess {
                    deployments: events.deployments,
                    result,
                    gas_used,
                    events: events.events,
                }),
                Err(message) => HostOutcome::Failure(HostFailure {
                    kind: HostFailureKind::State,
                    message,
                    gas_used,
                    failed_contract: request.contract,
                }),
            },
            Err(failure) => HostOutcome::Failure(failure),
        }
    }
}

pub fn code_hash(bytecode: &[u8]) -> [u8; 32] {
    Keccak256::digest(bytecode).into()
}

pub fn contract_address(
    deployer: Address,
    nonce: u64,
    code_hash: [u8; 32],
    chain_id: u64,
) -> Address {
    let mut hasher = Keccak256::new();
    hasher.update(b"LITHOVM_DEPLOY_V1");
    hasher.update(chain_id.to_be_bytes());
    hasher.update(deployer);
    hasher.update(nonce.to_be_bytes());
    hasher.update(code_hash);
    let digest = hasher.finalize();
    let mut address = [0; 32];
    address[12..].copy_from_slice(&digest[12..]);
    address
}

// Private dispatch keeps scalar APIs source-compatible while sharing atomicity,
// balance handling, code verification and recursive failure propagation.
trait HostValue: Clone + Sized {
    const DYNAMIC: bool;
    fn encoded_size(value: &Self) -> usize;
    fn execute(
        vm: &Vm,
        frame: &FrameRequest<Self>,
        contract: &mut DeployedContract,
        context: &ExecutionContext,
    ) -> ExecutionOutcome<Self>;
    fn execute_sync<T: StateTransaction>(
        _vm: &Vm,
        _state: &mut T,
        frame: FrameRequest<Self>,
        _contract: DeployedContract,
        _events: &mut EventJournal<Self>,
        _active: &mut Vec<Address>,
    ) -> Result<(ExecutionResult<Self>, u64), HostFailure> {
        Err(HostFailure {
            kind: HostFailureKind::InvalidBytecode,
            message: "synchronous execution requires typed host values".into(),
            gas_used: 0,
            failed_contract: frame.contract,
        })
    }
}

impl HostValue for [u8; 32] {
    const DYNAMIC: bool = false;
    fn encoded_size(_: &Self) -> usize {
        33
    }
    fn execute(
        vm: &Vm,
        frame: &FrameRequest<Self>,
        contract: &mut DeployedContract,
        context: &ExecutionContext,
    ) -> ExecutionOutcome<Self> {
        vm.execute_transactionally(
            &contract.bytecode,
            &frame.function,
            &frame.arguments,
            frame.gas_limit,
            &mut contract.storage,
            context,
        )
    }
}

impl HostValue for Value {
    fn execute_sync<T: StateTransaction>(
        vm: &Vm,
        state: &mut T,
        frame: FrameRequest<Self>,
        contract: DeployedContract,
        events: &mut EventJournal<Self>,
        active: &mut Vec<Address>,
    ) -> Result<(ExecutionResult<Self>, u64), HostFailure> {
        synchronous::execute(vm, state, frame, contract, events, active)
    }
    const DYNAMIC: bool = true;
    fn encoded_size(value: &Self) -> usize {
        match value {
            Value::Word(_, _) => 33,
            Value::String(text) => 3 + text.len(),
        }
    }
    fn execute(
        vm: &Vm,
        frame: &FrameRequest<Self>,
        contract: &mut DeployedContract,
        context: &ExecutionContext,
    ) -> ExecutionOutcome<Self> {
        vm.execute_values_transactionally(
            &contract.bytecode,
            &frame.function,
            &frame.arguments,
            frame.gas_limit,
            &mut contract.storage,
            context,
        )
    }
}

struct EventJournal<V> {
    deployments: Vec<CommittedDeployment>,
    events: Vec<CommittedEvent<V>>,
    value_bytes: usize,
}

struct FrameRequest<V> {
    contract: Address,
    function: String,
    arguments: Vec<V>,
    gas_limit: u64,
    caller: Address,
    value: [u8; 32],
    block_height: u64,
    block_timestamp: u64,
    chain_id: u64,
    depth: u16,
}

fn execute_frame<T: StateTransaction, V: HostValue>(
    vm: &Vm,
    state: &mut T,
    frame: FrameRequest<V>,
    committed_events: &mut EventJournal<V>,
    active_contracts: &mut Vec<Address>,
) -> Result<(ExecutionResult<V>, u64), HostFailure> {
    let mut contract = load_contract(state, frame.contract, 0)?;
    let program = parse(&contract.bytecode).map_err(|error| HostFailure {
        kind: HostFailureKind::InvalidBytecode,
        message: error.to_string(),
        gas_used: 0,
        failed_contract: frame.contract,
    })?;
    if program.bytecode_version() >= lithovm_bytecode::SYNC_VERSION {
        return V::execute_sync(
            vm,
            state,
            frame,
            contract,
            committed_events,
            active_contracts,
        );
    }
    let contract_balance = load_balance(state, frame.contract, 0, frame.contract)?;
    let context = ExecutionContext {
        caller: frame.caller,
        value: frame.value,
        block_height: frame.block_height,
        block_timestamp: frame.block_timestamp,
        chain_id: frame.chain_id,
        contract_balance,
        call_depth: frame.depth,
    };
    let result = match V::execute(vm, &frame, &mut contract, &context) {
        ExecutionOutcome::Success(result) => result,
        ExecutionOutcome::Failure(failure) => {
            return Err(vm_failure(frame.contract, failure));
        }
    };
    let mut total_gas = result.gas_used;

    // LithoVM calls are deferred effects. Persist the caller's storage before
    // entering children so reentrant frames observe checks/effects updates.
    store_contract(state, frame.contract, contract, total_gas)?;
    if V::DYNAMIC {
        for event in &result.events {
            let size = 7 + event
                .fields
                .iter()
                .map(|(_, _, value)| V::encoded_size(value))
                .sum::<usize>();
            if committed_events.value_bytes + size > lithovm_bytecode::values::MAX_ENVELOPE_BYTES {
                return Err(HostFailure {
                    kind: HostFailureKind::Vm(FailureKind::Trap),
                    message: "host event output exceeds byte limit".into(),
                    gas_used: total_gas,
                    failed_contract: frame.contract,
                });
            }
            committed_events.value_bytes += size;
        }
    }
    committed_events
        .events
        .extend(result.events.iter().cloned().map(|event| CommittedEvent {
            contract: frame.contract,
            event,
        }));

    for transfer in &result.transfers {
        let contract_balance = load_balance(state, frame.contract, total_gas, frame.contract)?;
        let contract_balance =
            debit(contract_balance, transfer.amount).ok_or_else(|| HostFailure {
                kind: HostFailureKind::State,
                message: "VM emitted a transfer exceeding the persisted contract balance".into(),
                gas_used: total_gas,
                failed_contract: frame.contract,
            })?;
        store_balance(
            state,
            frame.contract,
            contract_balance,
            total_gas,
            frame.contract,
        )?;
        let recipient_balance = load_balance(state, transfer.recipient, total_gas, frame.contract)?;
        let recipient_balance =
            credit(recipient_balance, transfer.amount).ok_or_else(|| HostFailure {
                kind: HostFailureKind::State,
                message: "native recipient balance overflow".into(),
                gas_used: total_gas,
                failed_contract: frame.contract,
            })?;
        store_balance(
            state,
            transfer.recipient,
            recipient_balance,
            total_gas,
            frame.contract,
        )?;
    }

    for call in &result.calls {
        if active_contracts.contains(&call.target) {
            return Err(HostFailure {
                kind: HostFailureKind::Reentrancy,
                message: "reentrant contract call is not permitted".into(),
                gas_used: total_gas,
                failed_contract: call.target,
            });
        }
        let contract_balance = load_balance(state, frame.contract, total_gas, frame.contract)?;
        let contract_balance = debit(contract_balance, call.value).ok_or_else(|| HostFailure {
            kind: HostFailureKind::State,
            message: "VM emitted a call exceeding the persisted contract balance".into(),
            gas_used: total_gas,
            failed_contract: frame.contract,
        })?;
        store_balance(
            state,
            frame.contract,
            contract_balance,
            total_gas,
            frame.contract,
        )?;
        let child = load_contract(state, call.target, total_gas)?;
        let function = child
            .entrypoints
            .get(&call.selector)
            .cloned()
            .ok_or_else(|| HostFailure {
                kind: HostFailureKind::UnknownSelector,
                message: "contract selector is not registered".into(),
                gas_used: total_gas,
                failed_contract: call.target,
            })?;
        let child_balance = load_balance(state, call.target, total_gas, call.target)?;
        let child_balance = credit(child_balance, call.value).ok_or_else(|| HostFailure {
            kind: HostFailureKind::State,
            message: "callee balance overflow".into(),
            gas_used: total_gas,
            failed_contract: call.target,
        })?;
        store_balance(state, call.target, child_balance, total_gas, call.target)?;
        let remaining_gas = frame.gas_limit.saturating_sub(total_gas);
        let child_frame = FrameRequest {
            contract: call.target,
            function,
            arguments: Vec::new(),
            gas_limit: remaining_gas,
            caller: frame.contract,
            value: call.value,
            block_height: frame.block_height,
            block_timestamp: frame.block_timestamp,
            chain_id: frame.chain_id,
            depth: call.depth,
        };
        active_contracts.push(call.target);
        let child_result =
            execute_frame(vm, state, child_frame, committed_events, active_contracts);
        active_contracts.pop();
        match child_result {
            Ok((_, child_gas)) => total_gas = total_gas.saturating_add(child_gas),
            Err(mut failure) => {
                failure.gas_used = total_gas
                    .saturating_add(failure.gas_used)
                    .min(frame.gas_limit);
                return Err(failure);
            }
        }
    }

    Ok((result, total_gas))
}

fn vm_failure(contract: Address, failure: ExecutionFailure) -> HostFailure {
    HostFailure {
        kind: HostFailureKind::Vm(failure.kind),
        message: failure.message,
        gas_used: failure.gas_used,
        failed_contract: contract,
    }
}

fn load_contract<T: StateTransaction>(
    state: &T,
    address: Address,
    gas_used: u64,
) -> Result<DeployedContract, HostFailure> {
    let contract = state
        .load_contract(&address)
        .map_err(|message| state_failure(message, gas_used, address))?
        .ok_or_else(|| HostFailure {
            kind: HostFailureKind::MissingContract,
            message: "contract is not deployed".into(),
            gas_used,
            failed_contract: address,
        })?;
    if code_hash(&contract.bytecode) != contract.code_hash {
        return Err(HostFailure {
            kind: HostFailureKind::State,
            message: "persisted contract code hash does not match bytecode".into(),
            gas_used,
            failed_contract: address,
        });
    }
    Ok(contract)
}

fn store_contract<T: StateTransaction>(
    state: &mut T,
    address: Address,
    contract: DeployedContract,
    gas_used: u64,
) -> Result<(), HostFailure> {
    state
        .store_contract(address, contract)
        .map_err(|message| state_failure(message, gas_used, address))
}

fn load_balance<T: StateTransaction>(
    state: &T,
    address: Address,
    gas_used: u64,
    contract: Address,
) -> Result<[u8; 32], HostFailure> {
    state
        .load_balance(&address)
        .map_err(|message| state_failure(message, gas_used, contract))
}

fn store_balance<T: StateTransaction>(
    state: &mut T,
    address: Address,
    balance: [u8; 32],
    gas_used: u64,
    contract: Address,
) -> Result<(), HostFailure> {
    state
        .store_balance(address, balance)
        .map_err(|message| state_failure(message, gas_used, contract))
}

fn state_failure(message: String, gas_used: u64, contract: Address) -> HostFailure {
    HostFailure {
        kind: HostFailureKind::State,
        message,
        gas_used,
        failed_contract: contract,
    }
}

fn debit(left: [u8; 32], right: [u8; 32]) -> Option<[u8; 32]> {
    if left < right {
        return None;
    }
    let mut result = [0; 32];
    let mut borrow = 0i16;
    for index in (0..32).rev() {
        let value = left[index] as i16 - right[index] as i16 - borrow;
        if value < 0 {
            result[index] = (value + 256) as u8;
            borrow = 1;
        } else {
            result[index] = value as u8;
            borrow = 0;
        }
    }
    Some(result)
}

fn credit(left: [u8; 32], right: [u8; 32]) -> Option<[u8; 32]> {
    let mut result = [0; 32];
    let mut carry = 0u16;
    for index in (0..32).rev() {
        let value = left[index] as u16 + right[index] as u16 + carry;
        result[index] = value as u8;
        carry = value >> 8;
    }
    (carry == 0).then_some(result)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InMemoryState {
    contracts: BTreeMap<Address, DeployedContract>,
    balances: BTreeMap<Address, [u8; 32]>,
}

impl InMemoryState {
    pub fn deploy(&mut self, address: Address, contract: DeployedContract) {
        self.contracts.insert(address, contract);
    }

    pub fn contract(&self, address: &Address) -> Option<&DeployedContract> {
        self.contracts.get(address)
    }

    pub fn balance(&self, address: &Address) -> [u8; 32] {
        self.balances.get(address).copied().unwrap_or([0; 32])
    }

    pub fn set_balance(&mut self, address: Address, balance: [u8; 32]) {
        self.balances.insert(address, balance);
    }
}

pub struct InMemoryTransaction<'a> {
    parent: &'a mut InMemoryState,
    staged: InMemoryState,
}

impl TransactionalState for InMemoryState {
    type Transaction<'a> = InMemoryTransaction<'a>;

    fn begin_transaction(&mut self) -> Result<Self::Transaction<'_>, String> {
        let staged = self.clone();
        Ok(InMemoryTransaction {
            parent: self,
            staged,
        })
    }
}

impl StateTransaction for InMemoryTransaction<'_> {
    fn load_contract(&self, address: &Address) -> Result<Option<DeployedContract>, String> {
        Ok(self.staged.contracts.get(address).cloned())
    }

    fn store_contract(
        &mut self,
        address: Address,
        contract: DeployedContract,
    ) -> Result<(), String> {
        self.staged.contracts.insert(address, contract);
        Ok(())
    }

    fn load_balance(&self, address: &Address) -> Result<[u8; 32], String> {
        Ok(self
            .staged
            .balances
            .get(address)
            .copied()
            .unwrap_or([0; 32]))
    }

    fn store_balance(&mut self, address: Address, balance: [u8; 32]) -> Result<(), String> {
        self.staged.balances.insert(address, balance);
        Ok(())
    }

    fn commit(self) -> Result<(), String> {
        *self.parent = self.staged;
        Ok(())
    }
}

#[cfg(test)]
mod entrypoint_tests {
    use super::*;

    #[test]
    fn selector_registration_rejects_duplicate_entries_with_contract_identity() {
        let artifact =
            lithic_lithovm::compile("contract Counter { pub fn get() -> u64 { return 1; } }")
                .unwrap();
        let bytecode = hex::decode(&artifact.bytecode[2..]).unwrap();
        let mut program = parse(&bytecode).unwrap();
        let address = [7; 32];
        let entries = validated_entrypoints(&program, address).unwrap();
        assert_eq!(entries.len(), 1);

        program.functions.push(program.functions[0].clone());
        let failure = validated_entrypoints(&program, address).unwrap_err();
        assert_eq!(failure.kind, HostFailureKind::InvalidEntrypoint);
        assert_eq!(failure.failed_contract, address);
        assert_eq!(failure.gas_used, 0);
    }
}
