use anyhow::{anyhow, bail, Result};
use lithovm_bytecode::values::{self, Value, MAX_STRING_BYTES};
use lithovm_bytecode::{
    parse, validate_word, Instruction, Program, ReturnValue, Statement, ValueType,
};
use lithovm_receipts::ReceiptV1;
use lithovm_zk_verifier::{StubVerifier, ZkVerifier};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::rc::Rc;
use std::sync::Arc;
mod persistence;

/// Scoped host fuel sink for the isolated chain adapter. Each VM charge is
/// forwarded before the corresponding instruction/effect proceeds. The hook
/// is thread-local because an FFI invocation is synchronous on its calling
/// thread; nested contract frames share the same sink.
#[derive(Clone, Copy)]
struct FuelHook {
    context: usize,
    charge: unsafe extern "C" fn(usize, u64) -> i32,
}

thread_local! {
    static FUEL_HOOK: Cell<Option<FuelHook>> = const { Cell::new(None) };
}

/// # Safety
/// `context` and `charge` must remain valid for the entire synchronous call;
/// the callback must not unwind or re-enter the same invocation.
pub unsafe fn with_external_fuel<T>(
    context: usize,
    charge: unsafe extern "C" fn(usize, u64) -> i32,
    run: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<FuelHook>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FUEL_HOOK.set(self.0);
        }
    }
    let previous = FUEL_HOOK.replace(Some(FuelHook { context, charge }));
    let _restore = Restore(previous);
    run()
}

/// Returns false only when the installed host sink rejects the charge.
/// Host-only charges (such as child-code creation) use this directly; VM
/// charges use GasMeter to keep local and external accounting in sync.
pub fn charge_external_fuel(amount: u64) -> bool {
    if amount == 0 {
        return true;
    }
    FUEL_HOOK.with(|hook| match hook.get() {
        Some(hook) => unsafe { (hook.charge)(hook.context, amount) == 1 },
        None => true,
    })
}

/// Minimal LithoVM execution context (scaffold).
pub struct Vm {
    verifier: Box<dyn ZkVerifier + Send + Sync>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionResult<T = [u8; 32]> {
    pub return_type: ValueType,
    pub return_value: T,
    pub gas_used: u64,
    pub events: Vec<EventRecord<T>>,
    pub transfers: Vec<NativeTransfer>,
    pub calls: Vec<ContractCall>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    InvalidRequest,
    Revert,
    OutOfGas,
    Trap,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionFailure {
    pub kind: FailureKind,
    pub message: String,
    pub gas_used: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionOutcome<T = [u8; 32]> {
    Success(ExecutionResult<T>),
    Failure(ExecutionFailure),
}

#[derive(Debug)]
struct ClassifiedFault {
    kind: FailureKind,
    message: String,
}

impl ClassifiedFault {
    fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl Display for ClassifiedFault {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ClassifiedFault {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord<T = [u8; 32]> {
    pub name: String,
    pub fields: Vec<(String, ValueType, T)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeTransfer {
    pub recipient: [u8; 32],
    pub amount: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractCall {
    pub target: [u8; 32],
    pub selector: [u8; 32],
    pub value: [u8; 32],
    pub depth: u16,
}

pub struct ValueCall<'a> {
    pub bytecode: &'a [u8],
    pub function: &'a str,
    pub arguments: &'a [Value],
    pub gas_limit: u64,
    pub context: &'a ExecutionContext,
}

pub struct NativeInvocation {
    pub target: [u8; 32],
    pub selector: [u8; 32],
    pub value: [u8; 32],
    pub arguments: Vec<Value>,
    pub return_type: ValueType,
    pub gas_limit: u64,
}

pub struct NativeCreation {
    pub template: [u8; 32],
    pub salt: [u8; 32],
    pub initializer: [u8; 32],
    pub value: [u8; 32],
    pub arguments: Vec<Value>,
    pub gas_limit: u64,
}

pub struct InvocationResult {
    pub value: Value,
    pub gas_used: u64,
    pub caller_balance: [u8; 32],
}

/// All effects must be staged in the same outer transaction. The adapter must
/// discard them on any VM failure; the VM cannot roll back external effects.
pub trait ExecutionHost {
    fn create(
        &mut self,
        _request: NativeCreation,
        _storage: &Storage,
    ) -> Result<InvocationResult, ExecutionFailure> {
        Err(ExecutionFailure {
            kind: FailureKind::Trap,
            message: "host does not support creation".into(),
            gas_used: 0,
        })
    }
    fn emit(&mut self, event: &EventRecord<Value>) -> Result<(), ExecutionFailure>;
    fn transfer(&mut self, transfer: &NativeTransfer) -> Result<[u8; 32], ExecutionFailure>;
    fn invoke(
        &mut self,
        request: NativeInvocation,
        storage: &Storage,
    ) -> Result<InvocationResult, ExecutionFailure>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecutionContext {
    pub caller: [u8; 32],
    pub value: [u8; 32],
    pub block_height: u64,
    pub block_timestamp: u64,
    pub chain_id: u64,
    pub contract_balance: [u8; 32],
    pub call_depth: u16,
}

/// Optional backend for lazily resolving map words. An absent key reads as zero.
/// The backend must never retain a callback outside its enclosing transaction.
pub trait MapReader: std::fmt::Debug {
    fn read_map(
        &self,
        field: &str,
        keys: &[[u8; 32]],
    ) -> std::result::Result<Option<[u8; 32]>, String>;
}

#[derive(Clone, Debug, Default)]
pub struct Storage {
    values: BTreeMap<String, [u8; 32]>,
    strings: BTreeMap<String, Arc<str>>,
    maps: BTreeMap<String, BTreeMap<Vec<[u8; 32]>, [u8; 32]>>,
    map_reader: Option<Rc<dyn MapReader>>,
}

impl PartialEq for Storage {
    fn eq(&self, other: &Self) -> bool {
        self.values == other.values && self.strings == other.strings && self.maps == other.maps
    }
}
impl Eq for Storage {}

impl Storage {
    pub fn attach_map_reader(&mut self, reader: Rc<dyn MapReader>) {
        self.map_reader = Some(reader);
    }

    pub fn staged_map_entries(&self) -> Vec<(String, Vec<[u8; 32]>, [u8; 32])> {
        self.maps
            .iter()
            .flat_map(|(field, entries)| {
                entries
                    .iter()
                    .map(|(keys, word)| (field.clone(), keys.clone(), *word))
            })
            .collect()
    }

    fn resolve_map(&self, field: &str, keys: &[[u8; 32]]) -> Result<Option<[u8; 32]>> {
        if let Some(word) = self.maps.get(field).and_then(|entries| entries.get(keys)) {
            return Ok(Some(*word));
        }
        match &self.map_reader {
            Some(reader) => reader.read_map(field, keys).map_err(anyhow::Error::msg),
            None => Ok(None),
        }
    }
}

impl Storage {
    pub fn get_string(&self, field: &str) -> Option<&str> {
        self.strings.get(field).map(AsRef::as_ref)
    }

    pub fn get(&self, field: &str) -> Option<&[u8; 32]> {
        self.values.get(field)
    }

    pub fn set_word(&mut self, field: impl Into<String>, word: [u8; 32]) {
        self.values.insert(field.into(), word);
    }

    pub fn get_map(&self, field: &str, keys: &[[u8; 32]]) -> Option<&[u8; 32]> {
        self.maps.get(field)?.get(keys)
    }

    pub fn set_map_word(&mut self, field: impl Into<String>, keys: Vec<[u8; 32]>, word: [u8; 32]) {
        self.maps
            .entry(field.into())
            .or_default()
            .insert(keys, word);
    }
}

pub const BASE_CALL_GAS: u64 = 10;
pub const PARAMETER_GAS: u64 = 2;
pub const INSTRUCTION_GAS: u64 = 1;
pub const NATIVE_TRANSFER_GAS: u64 = 20;
pub const CONTRACT_CALL_GAS: u64 = 40;
pub const MAP_READ_GAS: u64 = 25;
pub const MAP_WRITE_GAS: u64 = 100;
pub const MAX_CALL_DEPTH: u16 = 32;
pub const MAX_LOOP_ITERATIONS: u64 = 1024;

impl Default for Vm {
    fn default() -> Self {
        Self {
            verifier: Box::new(StubVerifier),
        }
    }
}

impl Vm {
    pub fn load_and_validate(&self, bytes: &[u8]) -> Result<()> {
        let _bc = parse(bytes)?;
        Ok(())
    }

    pub fn execute(
        &self,
        bytes: &[u8],
        function_name: &str,
        arguments: &[[u8; 32]],
        gas_limit: u64,
    ) -> Result<ExecutionResult> {
        let program = parse(bytes)?;
        if !program.storage.is_empty() || !program.maps.is_empty() {
            bail!("stateful LithoVM program requires execute_with_storage");
        }
        let mut storage = Storage::default();
        let mut gas_used = 0;
        execute_program(
            &program,
            function_name,
            arguments,
            gas_limit,
            &mut storage,
            None,
            &mut gas_used,
        )
    }

    pub fn execute_with_context(
        &self,
        bytes: &[u8],
        function_name: &str,
        arguments: &[[u8; 32]],
        gas_limit: u64,
        context: &ExecutionContext,
    ) -> Result<ExecutionResult> {
        let program = parse(bytes)?;
        if !program.storage.is_empty() || !program.maps.is_empty() {
            bail!("stateful LithoVM program requires execute_with_storage_and_context");
        }
        validate_context(context)?;
        let mut storage = Storage::default();
        let mut gas_used = 0;
        execute_program(
            &program,
            function_name,
            arguments,
            gas_limit,
            &mut storage,
            Some(context),
            &mut gas_used,
        )
    }

    pub fn execute_with_storage(
        &self,
        bytes: &[u8],
        function_name: &str,
        arguments: &[[u8; 32]],
        gas_limit: u64,
        storage: &mut Storage,
    ) -> Result<ExecutionResult> {
        let program = parse(bytes)?;
        let mut staged = storage.clone();
        prepare_storage(&program, &mut staged)?;
        let mut gas_used = 0;
        let result = execute_program(
            &program,
            function_name,
            arguments,
            gas_limit,
            &mut staged,
            None,
            &mut gas_used,
        )?;
        *storage = staged;
        Ok(result)
    }

    pub fn execute_with_storage_and_context(
        &self,
        bytes: &[u8],
        function_name: &str,
        arguments: &[[u8; 32]],
        gas_limit: u64,
        storage: &mut Storage,
        context: &ExecutionContext,
    ) -> Result<ExecutionResult> {
        let program = parse(bytes)?;
        validate_context(context)?;
        let mut staged = storage.clone();
        prepare_storage(&program, &mut staged)?;
        let mut gas_used = 0;
        let result = execute_program(
            &program,
            function_name,
            arguments,
            gas_limit,
            &mut staged,
            Some(context),
            &mut gas_used,
        )?;
        *storage = staged;
        Ok(result)
    }

    /// Execute a stateful call and return a structured, deterministic outcome.
    /// Storage is committed only when execution succeeds.
    pub fn execute_transactionally(
        &self,
        bytes: &[u8],
        function_name: &str,
        arguments: &[[u8; 32]],
        gas_limit: u64,
        storage: &mut Storage,
        context: &ExecutionContext,
    ) -> ExecutionOutcome {
        let mut gas_used = 0;
        let result = (|| {
            let program = parse(bytes)?;
            validate_context(context)?;
            let mut staged = storage.clone();
            prepare_storage(&program, &mut staged)?;
            let result = execute_program(
                &program,
                function_name,
                arguments,
                gas_limit,
                &mut staged,
                Some(context),
                &mut gas_used,
            )?;
            *storage = staged;
            Ok(result)
        })();
        match result {
            Ok(result) => ExecutionOutcome::Success(result),
            Err(error) => ExecutionOutcome::Failure(classify_failure(error, gas_used)),
        }
    }

    /// Candidate dynamic-value API. No chain transaction or host dispatch is implied.
    /// Both state and emitted effects are discarded on failure.
    pub fn execute_values_transactionally(
        &self,
        bytes: &[u8],
        function_name: &str,
        arguments: &[Value],
        gas_limit: u64,
        storage: &mut Storage,
        context: &ExecutionContext,
    ) -> ExecutionOutcome<Value> {
        self.execute_values_inner(
            ValueCall {
                bytecode: bytes,
                function: function_name,
                arguments,
                gas_limit,
                context,
            },
            storage,
            None,
        )
    }

    /// Candidate hosted execution (v13+). The enclosing host, not this method, owns
    /// atomic commit/rollback of external effects.
    pub fn execute_hosted(
        &self,
        call: ValueCall<'_>,
        storage: &mut Storage,
        host: &mut dyn ExecutionHost,
    ) -> ExecutionOutcome<Value> {
        self.execute_values_inner(call, storage, Some(host))
    }

    fn execute_values_inner(
        &self,
        call: ValueCall<'_>,
        storage: &mut Storage,
        host: Option<&mut dyn ExecutionHost>,
    ) -> ExecutionOutcome<Value> {
        let ValueCall {
            bytecode: bytes,
            function: function_name,
            arguments,
            gas_limit,
            context,
        } = call;
        let mut gas_used = 0;
        let result = (|| {
            let program = parse(bytes)?;
            if program.bytecode_version() >= lithovm_bytecode::SYNC_VERSION && host.is_none() {
                bail!("synchronous program requires an execution host");
            }
            if program.bytecode_version() < lithovm_bytecode::SYNC_VERSION && host.is_some() {
                bail!("hosted execution requires synchronous bytecode");
            }
            validate_context(context)?;
            // Validate individual and aggregate bounds before copying arguments.
            values::encoded_size(arguments)?;
            let function = program
                .functions
                .iter()
                .find(|f| f.name == function_name)
                .ok_or_else(|| anyhow!("unknown LithoVM function '{function_name}'"))?;
            if function.parameters.len() != arguments.len() {
                bail!("argument count mismatch");
            }
            for (ty, value) in function.parameters.iter().zip(arguments) {
                let actual = match value {
                    Value::Word(ty, _) => *ty,
                    Value::String(_) => ValueType::String,
                };
                if *ty != actual {
                    bail!("argument type mismatch");
                }
            }
            let initial_cost = BASE_CALL_GAS
                + PARAMETER_GAS * arguments.len() as u64
                + arguments
                    .iter()
                    .map(|value| match value {
                        Value::String(text) => text.len() as u64,
                        _ => 0,
                    })
                    .sum::<u64>();
            if gas_limit < initial_cost {
                charge_external_fuel(gas_limit);
                gas_used = gas_limit;
                return Err(ClassifiedFault::new(FailureKind::OutOfGas, "out of gas").into());
            }
            let arguments = arguments
                .iter()
                .map(StackValue::from_value)
                .collect::<Vec<_>>();
            let mut staged = storage.clone();
            prepare_storage(&program, &mut staged)?;
            let result = execute_dynamic_program(
                &program,
                function_name,
                &arguments,
                gas_limit,
                &mut staged,
                Some(context),
                &mut gas_used,
                host,
            )?;
            *storage = staged;
            Ok(result)
        })();
        match result {
            Ok(result) => ExecutionOutcome::Success(result),
            Err(error) => ExecutionOutcome::Failure(classify_failure(error, gas_used)),
        }
    }

    /// Validate a receipt signature/zk-proof at a high level (scaffold).
    /// Production: enforce LEP100-2/4/5 rules in consensus + runtime.
    pub fn validate_receipt(&self, _receipt: &ReceiptV1) -> Result<()> {
        Ok(())
    }

    pub fn verify_zk(
        &self,
        proof_system: &str,
        vk_id: &[u8],
        public_inputs: &[u8],
        proof: &[u8],
    ) -> Result<bool> {
        self.verifier
            .verify(proof_system, vk_id, public_inputs, proof)
    }
}

fn execute_program(
    program: &Program,
    function_name: &str,
    arguments: &[[u8; 32]],
    gas_limit: u64,
    storage: &mut Storage,
    context: Option<&ExecutionContext>,
    observed_gas: &mut u64,
) -> Result<ExecutionResult> {
    if program.bytecode_version() >= lithovm_bytecode::STRING_VERSION {
        bail!("dynamic program requires execute_values_transactionally");
    }
    let function = program
        .functions
        .iter()
        .find(|f| f.name == function_name)
        .ok_or_else(|| anyhow!("unknown LithoVM function '{function_name}'"))?;
    if function.parameters.len() != arguments.len() {
        bail!(
            "function '{}' requires {} arguments, received {}",
            function.name,
            function.parameters.len(),
            arguments.len()
        );
    }
    let arguments = arguments
        .iter()
        .zip(&function.parameters)
        .map(|(word, ty)| StackValue::scalar(*ty, *word))
        .collect::<Vec<_>>();
    let result = execute_dynamic_program(
        program,
        function_name,
        &arguments,
        gas_limit,
        storage,
        context,
        observed_gas,
        None,
    )?;
    let scalar = |value| match value {
        Value::Word(_, word) => Ok(word),
        Value::String(_) => Err(anyhow!("dynamic value in scalar execution")),
    };
    Ok(ExecutionResult {
        return_type: result.return_type,
        return_value: scalar(result.return_value)?,
        gas_used: result.gas_used,
        events: result
            .events
            .into_iter()
            .map(|event| {
                Ok(EventRecord {
                    name: event.name,
                    fields: event
                        .fields
                        .into_iter()
                        .map(|(name, ty, value)| Ok((name, ty, scalar(value)?)))
                        .collect::<Result<_>>()?,
                })
            })
            .collect::<Result<_>>()?,
        transfers: result.transfers,
        calls: result.calls,
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_dynamic_program(
    program: &Program,
    function_name: &str,
    arguments: &[StackValue],
    gas_limit: u64,
    storage: &mut Storage,
    context: Option<&ExecutionContext>,
    observed_gas: &mut u64,
    host: Option<&mut dyn ExecutionHost>,
) -> Result<ExecutionResult<Value>> {
    let function = program
        .functions
        .iter()
        .find(|function| function.name == function_name)
        .ok_or_else(|| anyhow!("unknown LithoVM function '{function_name}'"))?;
    if function.parameters.len() != arguments.len() {
        bail!(
            "function '{}' requires {} arguments, received {}",
            function.name,
            function.parameters.len(),
            arguments.len()
        );
    }
    for (index, (value_type, value)) in function.parameters.iter().zip(arguments).enumerate() {
        if *value_type != value.value_type {
            bail!("argument {index}: type mismatch");
        }
        if let Some(text) = &value.text {
            if text.len() > MAX_STRING_BYTES {
                bail!("string exceeds ABI byte limit");
            }
        } else {
            validate_word(*value_type, &value.word)?;
        }
    }
    let expression_cost = match &function.return_value {
        ReturnValue::Expression(instructions) => expression_gas(instructions),
        ReturnValue::Statements(_) => 0,
        _ => 0,
    };
    let gas_used = BASE_CALL_GAS
        .checked_add(PARAMETER_GAS.saturating_mul(arguments.len() as u64))
        .and_then(|gas| gas.checked_add(arguments.iter().map(StackValue::byte_cost).sum::<u64>()))
        .and_then(|gas| gas.checked_add(expression_cost))
        .ok_or_else(|| anyhow!("gas calculation overflow"))?;
    if gas_limit < gas_used {
        charge_external_fuel(gas_limit);
        *observed_gas = gas_limit;
        return Err(ClassifiedFault::new(
            FailureKind::OutOfGas,
            format!("out of gas: requires {gas_used}, limit is {gas_limit}"),
        )
        .into());
    }
    if !charge_external_fuel(gas_used) {
        *observed_gas = gas_limit;
        return Err(ClassifiedFault::new(FailureKind::OutOfGas, "host fuel exhausted").into());
    }
    *observed_gas = gas_used;
    let mut environment = RuntimeEnvironment {
        host,
        storage_fields: &program.storage,
        map_fields: &program.maps,
        event_definitions: &program.events,
        storage,
        context,
        events: Vec::new(),
        dynamic_event_bytes: 0,
        dynamic: program.bytecode_version() >= lithovm_bytecode::STRING_VERSION,
        transfers: Vec::new(),
        calls: Vec::new(),
        remaining_balance: context.map_or([0; 32], |value| value.contract_balance),
    };
    let return_value = match &function.return_value {
        ReturnValue::Constant(word) => StackValue::scalar(function.return_type, *word),
        ReturnValue::Parameter(index) => arguments[*index as usize].clone(),
        ReturnValue::Expression(instructions) => {
            return execute_expression(
                instructions,
                arguments,
                &function.parameters,
                function.return_type,
                GasMeter {
                    used: gas_used,
                    limit: gas_limit,
                },
                &environment,
                observed_gas,
            )
        }
        ReturnValue::Statements(statements) => {
            return execute_statements(
                statements,
                arguments,
                &function.parameters,
                function.return_type,
                GasMeter {
                    used: gas_used,
                    limit: gas_limit,
                },
                &mut environment,
                observed_gas,
            )
        }
    };
    let mut meter = GasMeter {
        used: gas_used,
        limit: gas_limit,
    };
    let charge = meter.charge(return_value.byte_cost());
    *observed_gas = meter.used;
    charge?;
    Ok(ExecutionResult {
        return_type: function.return_type,
        return_value: return_value.into_value(),
        gas_used: meter.used,
        events: environment.events,
        transfers: environment.transfers,
        calls: environment.calls,
    })
}

fn classify_failure(error: anyhow::Error, gas_used: u64) -> ExecutionFailure {
    let message = error.to_string();
    let kind = if let Some(fault) = error.downcast_ref::<ClassifiedFault>() {
        fault.kind
    } else if gas_used == 0 {
        FailureKind::InvalidRequest
    } else {
        FailureKind::Trap
    };
    ExecutionFailure {
        kind,
        message,
        gas_used,
    }
}

fn validate_context(context: &ExecutionContext) -> Result<()> {
    validate_word(ValueType::Address, &context.caller)
        .map_err(|error| anyhow!("execution context caller: {error}"))?;
    validate_word(ValueType::U256, &context.value)
        .map_err(|error| anyhow!("execution context value: {error}"))?;
    validate_word(ValueType::U256, &context.contract_balance)
        .map_err(|error| anyhow!("execution context contract balance: {error}"))?;
    if context.call_depth > MAX_CALL_DEPTH {
        bail!("execution context call depth exceeds {MAX_CALL_DEPTH}");
    }
    Ok(())
}

fn prepare_storage(program: &Program, storage: &mut Storage) -> Result<()> {
    for name in storage.values.keys() {
        if !program.storage.iter().any(|field| field.name == *name) {
            bail!("storage contains unknown field '{name}'");
        }
    }
    for (name, text) in &storage.strings {
        if !program
            .storage
            .iter()
            .any(|field| field.name == *name && field.value_type == ValueType::String)
            || text.len() > MAX_STRING_BYTES
        {
            bail!("invalid string storage field '{name}'");
        }
    }
    for field in &program.storage {
        if field.value_type == ValueType::String {
            if storage.values.contains_key(&field.name) {
                bail!("scalar value in string storage field '{}'", field.name);
            }
            storage
                .strings
                .entry(field.name.clone())
                .or_insert_with(|| Arc::from(""));
            continue;
        }
        let word = storage.values.entry(field.name.clone()).or_insert([0; 32]);
        validate_word(field.value_type, word)
            .map_err(|error| anyhow!("storage field '{}': {error}", field.name))?;
    }
    for name in storage.maps.keys() {
        if !program.maps.iter().any(|field| field.name == *name) {
            bail!("storage contains unknown map field '{name}'");
        }
    }
    for map in &program.maps {
        if let Some(entries) = storage.maps.get(&map.name) {
            for (keys, word) in entries {
                if keys.len() != map.key_types.len() {
                    bail!("storage map '{}' contains an invalid key", map.name);
                }
                for (key, key_type) in keys.iter().zip(&map.key_types) {
                    validate_word(*key_type, key)
                        .map_err(|error| anyhow!("storage map '{}': {error}", map.name))?;
                }
                validate_word(map.value_type, word)
                    .map_err(|error| anyhow!("storage map '{}': {error}", map.name))?;
            }
        }
    }
    Ok(())
}

#[derive(Clone)]
struct StackValue {
    value_type: ValueType,
    word: [u8; 32],
    text: Option<Arc<str>>,
}

impl StackValue {
    fn scalar(value_type: ValueType, word: [u8; 32]) -> Self {
        Self {
            value_type,
            word,
            text: None,
        }
    }

    fn from_value(value: &Value) -> Self {
        match value {
            Value::Word(ty, word) => Self::scalar(*ty, *word),
            Value::String(text) => Self {
                value_type: ValueType::String,
                word: [0; 32],
                text: Some(Arc::from(text.as_str())),
            },
        }
    }

    fn into_value(self) -> Value {
        match self.text {
            Some(text) => Value::String(text.to_string()),
            None => Value::Word(self.value_type, self.word),
        }
    }

    fn byte_cost(&self) -> u64 {
        self.text.as_ref().map_or(0, |text| text.len() as u64)
    }
}

struct RuntimeEnvironment<'a, 'h> {
    host: Option<&'h mut dyn ExecutionHost>,
    storage_fields: &'a [lithovm_bytecode::StorageField],
    map_fields: &'a [lithovm_bytecode::MapField],
    event_definitions: &'a [lithovm_bytecode::EventDefinition],
    storage: &'a mut Storage,
    context: Option<&'a ExecutionContext>,
    events: Vec<EventRecord<Value>>,
    dynamic_event_bytes: usize,
    dynamic: bool,
    transfers: Vec<NativeTransfer>,
    calls: Vec<ContractCall>,
    remaining_balance: [u8; 32],
}

fn execute_expression(
    instructions: &[Instruction],
    arguments: &[StackValue],
    parameter_types: &[ValueType],
    return_type: ValueType,
    mut meter: GasMeter,
    environment: &RuntimeEnvironment<'_, '_>,
    observed_gas: &mut u64,
) -> Result<ExecutionResult<Value>> {
    let result = (|| {
        let result = evaluate_expression(
            instructions,
            arguments,
            parameter_types,
            &[],
            environment,
            &mut meter,
        )?;
        meter.charge(result.byte_cost())?;
        Ok::<_, anyhow::Error>(result)
    })();
    *observed_gas = meter.used;
    let result = result?;
    if result.value_type != return_type {
        bail!("expression runtime type does not match function return type");
    }
    Ok(ExecutionResult {
        return_type,
        return_value: result.into_value(),
        gas_used: meter.used,
        events: Vec::new(),
        transfers: Vec::new(),
        calls: Vec::new(),
    })
}

fn evaluate_expression(
    instructions: &[Instruction],
    arguments: &[StackValue],
    _parameter_types: &[ValueType],
    locals: &[StackValue],
    environment: &RuntimeEnvironment<'_, '_>,
    meter: &mut GasMeter,
) -> Result<StackValue> {
    let mut stack: Vec<StackValue> = Vec::new();
    for instruction in instructions {
        match instruction {
            Instruction::Constant(value_type, word) => {
                stack.push(StackValue::scalar(*value_type, *word))
            }
            Instruction::Parameter(index) => {
                let value = &arguments[*index as usize];
                meter.charge(value.byte_cost())?;
                stack.push(value.clone());
            }
            Instruction::Local(index) => {
                let value = locals
                    .get(*index as usize)
                    .ok_or_else(|| anyhow!("runtime local index {index} is out of range"))?;
                meter.charge(value.byte_cost())?;
                stack.push(value.clone());
            }
            Instruction::Storage(index) => {
                let field = environment
                    .storage_fields
                    .get(*index as usize)
                    .ok_or_else(|| anyhow!("runtime storage index {index} is out of range"))?;
                if field.value_type == ValueType::String {
                    let text = environment
                        .storage
                        .strings
                        .get(&field.name)
                        .ok_or_else(|| anyhow!("runtime string storage is missing"))?;
                    meter.charge(text.len() as u64)?;
                    stack.push(StackValue {
                        value_type: ValueType::String,
                        word: [0; 32],
                        text: Some(text.clone()),
                    });
                    continue;
                }
                let word =
                    environment.storage.values.get(&field.name).ok_or_else(|| {
                        anyhow!("runtime storage field '{}' is missing", field.name)
                    })?;
                stack.push(StackValue::scalar(field.value_type, *word));
            }
            Instruction::MapStorage(index) => {
                let field = environment
                    .map_fields
                    .get(*index as usize)
                    .ok_or_else(|| anyhow!("runtime map index {index} is out of range"))?;
                let mut keys = Vec::with_capacity(field.key_types.len());
                for key_type in field.key_types.iter().rev() {
                    let key = stack
                        .pop()
                        .ok_or_else(|| anyhow!("expression stack underflow"))?;
                    if key.value_type != *key_type {
                        bail!("map key runtime type mismatch");
                    }
                    keys.push(key.word);
                }
                keys.reverse();
                let word = environment
                    .storage
                    .resolve_map(&field.name, &keys)?
                    .unwrap_or([0; 32]);
                validate_word(field.value_type, &word)?;
                stack.push(StackValue::scalar(field.value_type, word));
            }
            Instruction::MessageSender => {
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("contextual program requires an execution context"))?;
                stack.push(StackValue::scalar(ValueType::Address, context.caller));
            }
            Instruction::MessageValue => {
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("contextual program requires an execution context"))?;
                stack.push(StackValue::scalar(ValueType::U256, context.value));
            }
            Instruction::BlockHeight => {
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("contextual program requires an execution context"))?;
                stack.push(StackValue::scalar(
                    ValueType::U64,
                    word_from_u64(context.block_height),
                ));
            }
            Instruction::BlockTimestamp => {
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("contextual program requires an execution context"))?;
                stack.push(StackValue::scalar(
                    ValueType::U64,
                    word_from_u64(context.block_timestamp),
                ));
            }
            Instruction::ChainId => {
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("contextual program requires an execution context"))?;
                stack.push(StackValue::scalar(
                    ValueType::U64,
                    word_from_u64(context.chain_id),
                ));
            }
            Instruction::AddU64 => {
                binary_u64(&mut stack, u64::checked_add, "u64 addition overflow")?
            }
            Instruction::SubU64 => {
                binary_u64(&mut stack, u64::checked_sub, "u64 subtraction underflow")?
            }
            Instruction::MulU64 => {
                binary_u64(&mut stack, u64::checked_mul, "u64 multiplication overflow")?
            }
            Instruction::DivU64 => binary_u64(&mut stack, checked_div, "division by zero")?,
            Instruction::Eq => {
                let right = stack
                    .pop()
                    .ok_or_else(|| anyhow!("expression stack underflow"))?;
                let left = stack
                    .pop()
                    .ok_or_else(|| anyhow!("expression stack underflow"))?;
                if left.value_type != right.value_type {
                    bail!("equality operands have different runtime types");
                }
                meter.charge(left.byte_cost().saturating_add(right.byte_cost()))?;
                stack.push(StackValue::scalar(
                    ValueType::Bool,
                    word_from_bool(left.word == right.word && left.text == right.text),
                ));
            }
            Instruction::LtU64 => {
                let (left, right) = pop_u64_pair(&mut stack)?;
                stack.push(StackValue::scalar(
                    ValueType::Bool,
                    word_from_bool(left < right),
                ));
            }
            Instruction::AddU256 => {
                let (left, right) = pop_u256_pair(&mut stack)?;
                stack.push(StackValue::scalar(
                    ValueType::U256,
                    add_u256(left, right).ok_or_else(|| anyhow!("u256 addition overflow"))?,
                ));
            }
            Instruction::SubU256 => {
                let (left, right) = pop_u256_pair(&mut stack)?;
                stack.push(StackValue::scalar(
                    ValueType::U256,
                    subtract_u256(left, right)
                        .ok_or_else(|| anyhow!("u256 subtraction underflow"))?,
                ));
            }
            Instruction::GteU256 => {
                let (left, right) = pop_u256_pair(&mut stack)?;
                stack.push(StackValue::scalar(
                    ValueType::Bool,
                    word_from_bool(left >= right),
                ));
            }
        }
    }
    let result = stack
        .pop()
        .ok_or_else(|| anyhow!("expression produced no result"))?;
    if !stack.is_empty() {
        bail!("expression left extra runtime values");
    }
    Ok(result)
}

fn expression_gas(instructions: &[Instruction]) -> u64 {
    instructions.iter().fold(0u64, |gas, instruction| {
        gas.saturating_add(if matches!(instruction, Instruction::MapStorage(_)) {
            MAP_READ_GAS
        } else {
            INSTRUCTION_GAS
        })
    })
}

struct GasMeter {
    used: u64,
    limit: u64,
}

impl GasMeter {
    fn charge(&mut self, amount: u64) -> Result<()> {
        let attempted = self
            .used
            .checked_add(amount)
            .ok_or_else(|| anyhow!("gas calculation overflow"))?;
        if attempted > self.limit {
            charge_external_fuel(self.limit - self.used);
            self.used = self.limit;
            return Err(ClassifiedFault::new(
                FailureKind::OutOfGas,
                format!("out of gas: attempted {attempted}, limit is {}", self.limit),
            )
            .into());
        }
        if !charge_external_fuel(amount) {
            self.used = self.limit;
            return Err(ClassifiedFault::new(FailureKind::OutOfGas, "host fuel exhausted").into());
        }
        self.used = attempted;
        Ok(())
    }

    // Gas already debited by a synchronous child frame must still count
    // against the caller's local limit, without debiting the host twice.
    fn account_child(&mut self, amount: u64) -> Result<()> {
        let attempted = self
            .used
            .checked_add(amount)
            .ok_or_else(|| anyhow!("gas calculation overflow"))?;
        if attempted > self.limit {
            self.used = self.limit;
            return Err(
                ClassifiedFault::new(FailureKind::OutOfGas, "child exhausted caller gas").into(),
            );
        }
        self.used = attempted;
        Ok(())
    }
}

fn execute_statements(
    statements: &[Statement],
    arguments: &[StackValue],
    parameter_types: &[ValueType],
    return_type: ValueType,
    mut meter: GasMeter,
    environment: &mut RuntimeEnvironment<'_, '_>,
    observed_gas: &mut u64,
) -> Result<ExecutionResult<Value>> {
    if meter.used > meter.limit {
        bail!(
            "out of gas: requires at least {}, limit is {}",
            meter.used,
            meter.limit
        );
    }
    let mut locals = Vec::new();
    let result = execute_block(
        statements,
        arguments,
        parameter_types,
        &mut locals,
        &mut meter,
        environment,
    );
    *observed_gas = meter.used;
    let result = result?.ok_or_else(|| anyhow!("statement block completed without returning"))?;
    if result.value_type != return_type {
        bail!("statement return type does not match function return type");
    }
    let charge = meter.charge(result.byte_cost());
    *observed_gas = meter.used;
    charge?;
    Ok(ExecutionResult {
        return_type,
        return_value: result.into_value(),
        gas_used: meter.used,
        events: std::mem::take(&mut environment.events),
        transfers: std::mem::take(&mut environment.transfers),
        calls: std::mem::take(&mut environment.calls),
    })
}

fn execute_block(
    statements: &[Statement],
    arguments: &[StackValue],
    parameter_types: &[ValueType],
    locals: &mut Vec<StackValue>,
    meter: &mut GasMeter,
    environment: &mut RuntimeEnvironment<'_, '_>,
) -> Result<Option<StackValue>> {
    for statement in statements {
        meter.charge(INSTRUCTION_GAS)?;
        match statement {
            Statement::Create {
                template,
                salt,
                initializer,
                value,
                arguments: inputs,
            } => {
                meter.charge(CONTRACT_CALL_GAS)?;
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("creation requires context"))?;
                if context.call_depth >= MAX_CALL_DEPTH {
                    bail!("contract call depth limit reached");
                }
                let mut operands = Vec::with_capacity(4);
                for expression in [template, salt, initializer, value] {
                    meter.charge(expression_gas(expression))?;
                    operands.push(evaluate_expression(
                        expression,
                        arguments,
                        parameter_types,
                        locals,
                        environment,
                        meter,
                    )?);
                }
                let mut values = Vec::with_capacity(inputs.len());
                for (_, expression) in inputs {
                    meter.charge(expression_gas(expression))?;
                    let value = evaluate_expression(
                        expression,
                        arguments,
                        parameter_types,
                        locals,
                        environment,
                        meter,
                    )?;
                    meter.charge(value.byte_cost())?;
                    values.push(value.into_value());
                }
                values::encoded_size(&values)?;
                let request = NativeCreation {
                    template: operands[0].word,
                    salt: operands[1].word,
                    initializer: operands[2].word,
                    value: operands[3].word,
                    arguments: values,
                    gas_limit: meter.limit - meter.used,
                };
                let host = environment
                    .host
                    .as_mut()
                    .ok_or_else(|| anyhow!("creation requires execution host"))?;
                let returned = match host.create(request, environment.storage) {
                    Ok(result) => {
                        meter.account_child(result.gas_used)?;
                        result
                    }
                    Err(failure) => {
                        meter.account_child(failure.gas_used)?;
                        return Err(ClassifiedFault::new(failure.kind, failure.message).into());
                    }
                };
                values::encoded_size(std::slice::from_ref(&returned.value))?;
                let Value::Word(ValueType::Address, address) = returned.value else {
                    bail!("creation must return address");
                };
                environment.remaining_balance = returned.caller_balance;
                locals.push(StackValue::scalar(ValueType::Address, address));
            }
            Statement::Invoke {
                return_type,
                target,
                selector,
                value,
                arguments: inputs,
            } => {
                meter.charge(CONTRACT_CALL_GAS)?;
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("invoke requires context"))?;
                if context.call_depth >= MAX_CALL_DEPTH {
                    bail!("contract call depth limit reached");
                }
                let mut operands = Vec::with_capacity(3);
                for expression in [target, selector, value] {
                    meter.charge(expression_gas(expression))?;
                    operands.push(evaluate_expression(
                        expression,
                        arguments,
                        parameter_types,
                        locals,
                        environment,
                        meter,
                    )?);
                }
                let mut values = Vec::with_capacity(inputs.len());
                for (_, expression) in inputs {
                    meter.charge(expression_gas(expression))?;
                    let value = evaluate_expression(
                        expression,
                        arguments,
                        parameter_types,
                        locals,
                        environment,
                        meter,
                    )?;
                    meter.charge(value.byte_cost())?;
                    values.push(value.into_value());
                }
                values::encoded_size(&values)?;
                let request = NativeInvocation {
                    target: operands[0].word,
                    selector: operands[1].word,
                    value: operands[2].word,
                    arguments: values,
                    return_type: *return_type,
                    gas_limit: meter.limit - meter.used,
                };
                let host = environment
                    .host
                    .as_mut()
                    .ok_or_else(|| anyhow!("invoke requires an execution host"))?;
                let returned = match host.invoke(request, environment.storage) {
                    Ok(result) => {
                        meter.account_child(result.gas_used)?;
                        result
                    }
                    Err(failure) => {
                        meter.account_child(failure.gas_used)?;
                        return Err(ClassifiedFault::new(failure.kind, failure.message).into());
                    }
                };
                values::encoded_size(std::slice::from_ref(&returned.value))?;
                if let Value::String(text) = &returned.value {
                    meter.charge(text.len() as u64)?;
                }
                let returned_value = StackValue::from_value(&returned.value);
                if returned_value.value_type != *return_type {
                    bail!("invoke return type mismatch");
                }
                environment.remaining_balance = returned.caller_balance;
                locals.push(returned_value);
            }
            Statement::Let {
                value_type,
                expression,
            }
            | Statement::LetMutable {
                value_type,
                expression,
            } => {
                meter.charge(expression_gas(expression))?;
                let value = evaluate_expression(
                    expression,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if value.value_type != *value_type {
                    bail!("local binding runtime type mismatch");
                }
                locals.push(value);
            }
            Statement::SetLocal { local, expression } => {
                meter.charge(expression_gas(expression))?;
                let value = evaluate_expression(
                    expression,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                let binding = locals
                    .get_mut(*local as usize)
                    .ok_or_else(|| anyhow!("runtime local assignment index is out of range"))?;
                if binding.value_type != value.value_type {
                    bail!("local assignment runtime type mismatch");
                }
                *binding = value;
            }
            Statement::Repeat { count, body } => {
                meter.charge(expression_gas(count))?;
                let count = evaluate_expression(
                    count,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if count.value_type != ValueType::U64 || count.word[..24] != [0; 24] {
                    bail!("repeat count runtime type is not a canonical u64");
                }
                let count = u64::from_be_bytes(count.word[24..].try_into().unwrap());
                if count > MAX_LOOP_ITERATIONS {
                    bail!("repeat count exceeds {MAX_LOOP_ITERATIONS}");
                }
                let local_count = locals.len();
                for _ in 0..count {
                    let result = execute_block(
                        body,
                        arguments,
                        parameter_types,
                        locals,
                        meter,
                        environment,
                    )?;
                    locals.truncate(local_count);
                    if result.is_some() {
                        return Ok(result);
                    }
                }
            }
            Statement::Require(condition) => {
                meter.charge(expression_gas(condition))?;
                let condition = evaluate_expression(
                    condition,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if condition.value_type != ValueType::Bool {
                    bail!("require condition runtime type is not bool");
                }
                if condition.word == word_from_bool(false) {
                    return Err(ClassifiedFault::new(
                        FailureKind::Revert,
                        "require condition failed",
                    )
                    .into());
                }
                if condition.word != word_from_bool(true) {
                    bail!("require condition is not a canonical bool");
                }
            }
            Statement::Revert => {
                return Err(ClassifiedFault::new(FailureKind::Revert, "execution reverted").into())
            }
            Statement::Return(expression) => {
                meter.charge(expression_gas(expression))?;
                return Ok(Some(evaluate_expression(
                    expression,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?));
            }
            Statement::Store { field, expression } => {
                meter.charge(expression_gas(expression))?;
                let field = environment
                    .storage_fields
                    .get(*field as usize)
                    .ok_or_else(|| anyhow!("runtime store field index is out of range"))?;
                let value = evaluate_expression(
                    expression,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if value.value_type != field.value_type {
                    bail!("storage write runtime type mismatch");
                }
                if let Some(text) = value.text {
                    meter.charge(text.len() as u64)?;
                    environment.storage.strings.insert(field.name.clone(), text);
                    continue;
                }
                validate_word(field.value_type, &value.word)?;
                environment
                    .storage
                    .values
                    .insert(field.name.clone(), value.word);
            }
            Statement::MapStore {
                map,
                keys,
                expression,
            } => {
                let field = environment
                    .map_fields
                    .get(*map as usize)
                    .ok_or_else(|| anyhow!("runtime map store index is out of range"))?;
                let mut evaluated_keys = Vec::with_capacity(keys.len());
                for (key, key_type) in keys.iter().zip(&field.key_types) {
                    meter.charge(expression_gas(key))?;
                    let key = evaluate_expression(
                        key,
                        arguments,
                        parameter_types,
                        locals,
                        environment,
                        meter,
                    )?;
                    if key.value_type != *key_type {
                        bail!("map key runtime type mismatch");
                    }
                    evaluated_keys.push(key.word);
                }
                meter.charge(expression_gas(expression))?;
                let value = evaluate_expression(
                    expression,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if value.value_type != field.value_type {
                    bail!("map value runtime type mismatch");
                }
                meter.charge(MAP_WRITE_GAS)?;
                environment
                    .storage
                    .maps
                    .entry(field.name.clone())
                    .or_default()
                    .insert(evaluated_keys, value.word);
            }
            Statement::Emit { event, values } => {
                let definition = environment
                    .event_definitions
                    .get(*event as usize)
                    .ok_or_else(|| anyhow!("runtime event index is out of range"))?;
                let mut fields = Vec::with_capacity(values.len());
                let mut event_bytes = 7usize;
                for (expression, field) in values.iter().zip(&definition.fields) {
                    meter.charge(expression_gas(expression))?;
                    let value = evaluate_expression(
                        expression,
                        arguments,
                        parameter_types,
                        locals,
                        environment,
                        meter,
                    )?;
                    if value.value_type != field.value_type {
                        bail!("event field runtime type mismatch");
                    }
                    if value.text.is_some() {
                        event_bytes += 3 + value.byte_cost() as usize;
                    } else {
                        event_bytes += 33;
                    }
                    if environment.dynamic
                        && environment.dynamic_event_bytes + event_bytes
                            > values::MAX_ENVELOPE_BYTES
                    {
                        bail!("dynamic event output exceeds byte limit");
                    }
                    meter.charge(value.byte_cost())?;
                    fields.push((field.name.clone(), field.value_type, value.into_value()));
                }
                if environment.dynamic {
                    if environment.dynamic_event_bytes + event_bytes > values::MAX_ENVELOPE_BYTES {
                        bail!("dynamic event output exceeds byte limit");
                    }
                    environment.dynamic_event_bytes += event_bytes;
                }
                let record = EventRecord {
                    name: definition.name.clone(),
                    fields,
                };
                if let Some(host) = environment.host.as_mut() {
                    host.emit(&record)
                        .map_err(|failure| ClassifiedFault::new(failure.kind, failure.message))?;
                }
                environment.events.push(record);
            }
            Statement::Transfer { recipient, amount } => {
                meter.charge(NATIVE_TRANSFER_GAS)?;
                meter.charge(expression_gas(recipient))?;
                let recipient = evaluate_expression(
                    recipient,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                meter.charge(expression_gas(amount))?;
                let amount = evaluate_expression(
                    amount,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if recipient.value_type != ValueType::Address
                    || amount.value_type != ValueType::U256
                {
                    bail!("native transfer runtime type mismatch");
                }
                let _ = environment
                    .context
                    .ok_or_else(|| anyhow!("native transfer requires an execution context"))?;
                environment.remaining_balance =
                    subtract_u256(environment.remaining_balance, amount.word)
                        .ok_or_else(|| anyhow!("native transfer exceeds contract balance"))?;
                let transfer = NativeTransfer {
                    recipient: recipient.word,
                    amount: amount.word,
                };
                if let Some(host) = environment.host.as_mut() {
                    environment.remaining_balance = host
                        .transfer(&transfer)
                        .map_err(|failure| ClassifiedFault::new(failure.kind, failure.message))?;
                } else {
                    environment.transfers.push(transfer);
                }
            }
            Statement::Call {
                target,
                selector,
                value,
            } => {
                meter.charge(CONTRACT_CALL_GAS)?;
                meter.charge(expression_gas(target))?;
                let target = evaluate_expression(
                    target,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                meter.charge(expression_gas(selector))?;
                let selector = evaluate_expression(
                    selector,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                meter.charge(expression_gas(value))?;
                let value = evaluate_expression(
                    value,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if target.value_type != ValueType::Address
                    || selector.value_type != ValueType::Bytes32
                    || value.value_type != ValueType::U256
                {
                    bail!("contract call runtime type mismatch");
                }
                let context = environment
                    .context
                    .ok_or_else(|| anyhow!("contract call requires an execution context"))?;
                if context.call_depth >= MAX_CALL_DEPTH {
                    bail!("contract call depth limit reached");
                }
                environment.remaining_balance =
                    subtract_u256(environment.remaining_balance, value.word)
                        .ok_or_else(|| anyhow!("contract call value exceeds contract balance"))?;
                environment.calls.push(ContractCall {
                    target: target.word,
                    selector: selector.word,
                    value: value.word,
                    depth: context.call_depth + 1,
                });
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
            } => {
                meter.charge(expression_gas(condition))?;
                let condition = evaluate_expression(
                    condition,
                    arguments,
                    parameter_types,
                    locals,
                    environment,
                    meter,
                )?;
                if condition.value_type != ValueType::Bool {
                    bail!("if condition runtime type is not bool");
                }
                let local_count = locals.len();
                let branch = if condition.word == word_from_bool(true) {
                    then_branch
                } else if condition.word == word_from_bool(false) {
                    else_branch
                } else {
                    bail!("if condition is not a canonical bool");
                };
                let result = execute_block(
                    branch,
                    arguments,
                    parameter_types,
                    locals,
                    meter,
                    environment,
                )?;
                locals.truncate(local_count);
                if result.is_some() {
                    return Ok(result);
                }
            }
        }
    }
    Ok(None)
}

fn binary_u64(
    stack: &mut Vec<StackValue>,
    operation: fn(u64, u64) -> Option<u64>,
    failure: &str,
) -> Result<()> {
    let (left, right) = pop_u64_pair(stack)?;
    let value = operation(left, right).ok_or_else(|| anyhow!(failure.to_string()))?;
    stack.push(StackValue::scalar(ValueType::U64, word_from_u64(value)));
    Ok(())
}

fn pop_u64_pair(stack: &mut Vec<StackValue>) -> Result<(u64, u64)> {
    let right = pop_u64(stack)?;
    let left = pop_u64(stack)?;
    Ok((left, right))
}

fn pop_u64(stack: &mut Vec<StackValue>) -> Result<u64> {
    let value = stack
        .pop()
        .ok_or_else(|| anyhow!("expression stack underflow"))?;
    if value.value_type != ValueType::U64 || value.word[..24] != [0; 24] {
        bail!("expression operand is not a canonical u64");
    }
    Ok(u64::from_be_bytes(value.word[24..].try_into().unwrap()))
}

fn pop_u256_pair(stack: &mut Vec<StackValue>) -> Result<([u8; 32], [u8; 32])> {
    let right = stack
        .pop()
        .ok_or_else(|| anyhow!("expression stack underflow"))?;
    let left = stack
        .pop()
        .ok_or_else(|| anyhow!("expression stack underflow"))?;
    if left.value_type != ValueType::U256 || right.value_type != ValueType::U256 {
        bail!("expression operands are not u256");
    }
    Ok((left.word, right.word))
}

fn checked_div(left: u64, right: u64) -> Option<u64> {
    left.checked_div(right)
}

fn word_from_u64(value: u64) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

fn word_from_bool(value: bool) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[31] = u8::from(value);
    word
}

fn subtract_u256(left: [u8; 32], right: [u8; 32]) -> Option<[u8; 32]> {
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

fn add_u256(left: [u8; 32], right: [u8; 32]) -> Option<[u8; 32]> {
    let mut result = [0; 32];
    let mut carry = 0u16;
    for index in (0..32).rev() {
        let value = left[index] as u16 + right[index] as u16 + carry;
        result[index] = value as u8;
        carry = value >> 8;
    }
    (carry == 0).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lithovm_bytecode::{Function, Program};

    unsafe extern "C" fn debit_test_fuel(context: usize, amount: u64) -> i32 {
        let remaining = unsafe { &*(context as *const Cell<u64>) };
        if amount > remaining.get() {
            return 0;
        }
        remaining.set(remaining.get() - amount);
        1
    }

    #[test]
    fn scoped_live_fuel_rejects_before_execution_and_restores_on_exit() {
        let bytes = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "answer".into(),
                parameters: vec![],
                return_type: ValueType::U64,
                return_value: ReturnValue::Constant(u64_word(42)),
            }],
        }
        .encode()
        .unwrap();
        let vm = Vm::default();
        let remaining = Cell::new(BASE_CALL_GAS);
        unsafe {
            with_external_fuel(&remaining as *const _ as usize, debit_test_fuel, || {
                assert!(vm.execute(&bytes, "answer", &[], 100).is_ok());
            })
        };
        assert_eq!(remaining.get(), 0);
        let remaining = Cell::new(BASE_CALL_GAS - 1);
        unsafe {
            with_external_fuel(&remaining as *const _ as usize, debit_test_fuel, || {
                assert!(vm.execute(&bytes, "answer", &[], 100).is_err());
            })
        };
        assert_eq!(remaining.get(), BASE_CALL_GAS - 1);
        assert!(vm.execute(&bytes, "answer", &[], 100).is_ok());
    }

    fn u64_word(value: u64) -> [u8; 32] {
        let mut word = [0u8; 32];
        word[24..].copy_from_slice(&value.to_be_bytes());
        word
    }

    #[test]
    fn u256_balance_subtraction_checks_underflow() {
        assert_eq!(subtract_u256([0xff; 32], [0xff; 32]), Some([0; 32]));
        let mut one = [0; 32];
        one[31] = 1;
        assert_eq!(subtract_u256([0; 32], one), None);
        let mut high = [0; 32];
        high[0] = 1;
        let mut expected = [0xff; 32];
        expected[0] = 0;
        assert_eq!(subtract_u256(high, one), Some(expected));
    }

    #[test]
    fn executes_constant_and_identity_functions() {
        let bytes = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![
                Function {
                    name: "answer".into(),
                    parameters: vec![],
                    return_type: ValueType::U64,
                    return_value: ReturnValue::Constant(u64_word(42)),
                },
                Function {
                    name: "echo".into(),
                    parameters: vec![ValueType::U64],
                    return_type: ValueType::U64,
                    return_value: ReturnValue::Parameter(0),
                },
            ],
        }
        .encode()
        .unwrap();
        let vm = Vm::default();
        assert_eq!(
            vm.execute(&bytes, "answer", &[], BASE_CALL_GAS)
                .unwrap()
                .return_value,
            u64_word(42)
        );
        assert_eq!(
            vm.execute(
                &bytes,
                "echo",
                &[u64_word(9005)],
                BASE_CALL_GAS + PARAMETER_GAS
            )
            .unwrap()
            .return_value,
            u64_word(9005)
        );
    }

    #[test]
    fn rejects_bad_calls_before_execution() {
        let bytes = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "echo".into(),
                parameters: vec![ValueType::Bool],
                return_type: ValueType::Bool,
                return_value: ReturnValue::Parameter(0),
            }],
        }
        .encode()
        .unwrap();
        let vm = Vm::default();
        assert!(vm.execute(&bytes, "missing", &[], 100).is_err());
        assert!(vm.execute(&bytes, "echo", &[], 100).is_err());
        assert!(vm.execute(&bytes, "echo", &[[2; 32]], 100).is_err());
        assert!(vm.execute(&bytes, "echo", &[[0; 32]], 0).is_err());
    }

    #[test]
    fn transactional_outcomes_report_failure_kind_and_consumed_gas() {
        let bytes = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "guarded".into(),
                parameters: vec![],
                return_type: ValueType::U64,
                return_value: ReturnValue::Statements(vec![
                    Statement::Require(vec![Instruction::Constant(
                        ValueType::Bool,
                        word_from_bool(false),
                    )]),
                    Statement::Return(vec![Instruction::Constant(ValueType::U64, u64_word(1))]),
                ]),
            }],
        }
        .encode()
        .unwrap();
        let vm = Vm::default();
        let context = ExecutionContext::default();
        let mut storage = Storage::default();

        assert_eq!(
            vm.execute_transactionally(&bytes, "missing", &[], 100, &mut storage, &context),
            ExecutionOutcome::Failure(ExecutionFailure {
                kind: FailureKind::InvalidRequest,
                message: "unknown LithoVM function 'missing'".into(),
                gas_used: 0,
            })
        );

        let ExecutionOutcome::Failure(reverted) =
            vm.execute_transactionally(&bytes, "guarded", &[], 100, &mut storage, &context)
        else {
            panic!("guarded call should revert");
        };
        assert_eq!(reverted.kind, FailureKind::Revert);
        assert_eq!(reverted.gas_used, BASE_CALL_GAS + 2 * INSTRUCTION_GAS);

        let ExecutionOutcome::Failure(out_of_gas) = vm.execute_transactionally(
            &bytes,
            "guarded",
            &[],
            BASE_CALL_GAS,
            &mut storage,
            &context,
        ) else {
            panic!("guarded call should run out of gas");
        };
        assert_eq!(out_of_gas.kind, FailureKind::OutOfGas);
        assert_eq!(out_of_gas.gas_used, BASE_CALL_GAS);
    }

    #[test]
    fn executes_checked_typed_expressions() {
        let bytes = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "increment".into(),
                parameters: vec![ValueType::U64],
                return_type: ValueType::U64,
                return_value: ReturnValue::Expression(vec![
                    Instruction::Parameter(0),
                    Instruction::Constant(ValueType::U64, u64_word(1)),
                    Instruction::AddU64,
                ]),
            }],
        }
        .encode()
        .unwrap();
        let gas = BASE_CALL_GAS + PARAMETER_GAS + 3 * INSTRUCTION_GAS;
        assert_eq!(
            Vm::default()
                .execute(&bytes, "increment", &[u64_word(41)], gas)
                .unwrap()
                .return_value,
            u64_word(42)
        );
        assert!(Vm::default()
            .execute(&bytes, "increment", &[u64_word(u64::MAX)], gas)
            .is_err());
    }
}
