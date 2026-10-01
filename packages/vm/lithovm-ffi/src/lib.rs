//! Isolated nonpayable chain harness. No live precompile registration.
use lithovm::{MapReader, Storage};
use lithovm_bytecode::{
    function_selector, parse,
    values::{self, Value},
};
use lithovm_host::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as Json};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_WRITES: usize = 128;
type ReadFn = unsafe extern "C" fn(usize, *const u8, usize, *mut u8, usize) -> isize;
type ChargeFn = unsafe extern "C" fn(usize, u64) -> i32;
#[repr(C)]
pub struct Buffer {
    pub data: *mut u8,
    pub len: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    operation: String,
    caller: String,
    contract: Option<String>,
    bytecode: Option<String>,
    function: Option<String>,
    selector: Option<String>,
    arguments: String,
    nonce: u64,
    gas_limit: u64,
    chain_id: u64,
    block_height: u64,
    block_timestamp: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    version: u8,
    bytecode: String,
    code_hash: String,
    storage: String,
}

fn hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(bytes))
}
fn decode(s: &str) -> Result<Vec<u8>, String> {
    hex::decode(s.strip_prefix("0x").ok_or("hex prefix required")?).map_err(|e| e.to_string())
}
fn address(s: &str) -> Result<Address, String> {
    let bytes = decode(s)?;
    if bytes.len() != 20 {
        return Err("address must be 20 bytes".into());
    }
    let mut word = [0; 32];
    word[12..].copy_from_slice(&bytes);
    Ok(word)
}
fn key(address: &Address) -> String {
    format!("lithovm/v1/contracts/{}", hex::encode(&address[12..]))
}

fn map_key(address: &Address, field: &str, keys: &[[u8; 32]]) -> Result<String, String> {
    if field.is_empty()
        || field.len() > 255
        || !field.as_bytes()[0].is_ascii_alphabetic() && field.as_bytes()[0] != b'_'
        || !field
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || keys.is_empty()
        || keys.len() > 64
    {
        return Err("invalid persistent map key".into());
    }
    let mut encoded = String::with_capacity(keys.len() * 64);
    for word in keys {
        encoded.push_str(&hex::encode(word));
    }
    Ok(format!(
        "lithovm/v2/maps/{}/{field}/{encoded}",
        hex::encode(&address[12..])
    ))
}

fn selected_function(
    function: Option<String>,
    selector: Option<String>,
    entrypoints: impl IntoIterator<Item = ([u8; 32], String)>,
) -> Result<Option<String>, String> {
    match (function, selector) {
        (Some(name), None) => Ok(Some(name)),
        (None, None) => Ok(None),
        (None, Some(selector)) => {
            let bytes = decode(&selector)?;
            if bytes.len() != 32 {
                return Err("native selector must be 32 bytes".into());
            }
            if bytes.iter().all(|byte| *byte == 0) {
                return Ok(None);
            }
            let name = entrypoints
                .into_iter()
                .find(|(candidate, _)| candidate.as_slice() == bytes.as_slice())
                .map(|(_, name)| name)
                .ok_or("unknown native selector")?;
            Ok(Some(name))
        }
        (Some(_), Some(_)) => Err("function and selector are mutually exclusive".into()),
    }
}

struct CallbackState {
    read: ReadFn,
    context: usize,
    committed: BTreeMap<String, Vec<u8>>,
    read_bytes: Rc<Cell<usize>>,
    read_count: Rc<Cell<usize>>,
}
struct Transaction<'a> {
    parent: &'a mut CallbackState,
    writes: BTreeMap<String, Vec<u8>>,
    map_writes: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
}
impl TransactionalState for CallbackState {
    type Transaction<'a> = Transaction<'a>;
    fn begin_transaction(&mut self) -> Result<Transaction<'_>, String> {
        Ok(Transaction {
            parent: self,
            writes: BTreeMap::new(),
            map_writes: Rc::new(RefCell::new(BTreeMap::new())),
        })
    }
}
impl CallbackState {
    fn read(&self, key: &str) -> Result<Option<Vec<u8>>, String> {
        read_callback(
            self.read,
            self.context,
            &self.read_count,
            &self.read_bytes,
            key,
        )
    }
}

fn read_callback(
    read: ReadFn,
    context: usize,
    read_count: &Cell<usize>,
    read_bytes: &Cell<usize>,
    key: &str,
) -> Result<Option<Vec<u8>>, String> {
    let count = read_count.get() + 1;
    read_count.set(count);
    if count > 256 {
        return Err("state read count exceeds harness limit".into());
    }
    // Callback memory is valid only for this synchronous call. No pointer is retained.
    let size = unsafe { (read)(context, key.as_ptr(), key.len(), std::ptr::null_mut(), 0) };
    if size == -1 {
        return Ok(None);
    }
    if size < 0 || size as usize > MAX_BYTES {
        return Err("state read failed or exceeds limit".into());
    }
    let total = read_bytes.get() + size as usize;
    if total > 8 * MAX_BYTES {
        return Err("aggregate state reads exceed harness limit".into());
    }
    read_bytes.set(total);
    let mut bytes = vec![0; size as usize];
    let actual = unsafe {
        (read)(
            context,
            key.as_ptr(),
            key.len(),
            bytes.as_mut_ptr(),
            bytes.len(),
        )
    };
    if actual != size {
        return Err("state read changed during execution".into());
    }
    Ok(Some(bytes))
}

#[derive(Debug)]
struct CallbackMapReader {
    read: ReadFn,
    context: usize,
    address: Address,
    read_count: Rc<Cell<usize>>,
    read_bytes: Rc<Cell<usize>>,
    staged: Rc<RefCell<BTreeMap<String, Vec<u8>>>>,
}

impl MapReader for CallbackMapReader {
    fn read_map(&self, field: &str, keys: &[[u8; 32]]) -> Result<Option<[u8; 32]>, String> {
        let key = map_key(&self.address, field, keys)?;
        let bytes = match self.staged.borrow().get(&key) {
            Some(value) => Some(value.clone()),
            None => read_callback(
                self.read,
                self.context,
                &self.read_count,
                &self.read_bytes,
                &key,
            )?,
        };
        match bytes {
            Some(bytes) if bytes.is_empty() => Ok(None),
            Some(bytes) if bytes.len() == 32 => Ok(Some(bytes.try_into().unwrap())),
            Some(_) => Err("invalid persistent map value length".into()),
            None => Ok(None),
        }
    }
}
impl StateTransaction for Transaction<'_> {
    fn load_contract(&self, address: &Address) -> Result<Option<DeployedContract>, String> {
        let key = key(address);
        let bytes = match self.writes.get(&key) {
            Some(v) => Some(v.clone()),
            None => self.parent.read(&key)?,
        };
        let Some(bytes) = bytes else { return Ok(None) };
        let record: Stored = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if record.version != 2 {
            return Err("unsupported contract record; explicit migration required".into());
        }
        let code = decode(&record.bytecode)?;
        if code.len() > MAX_CHILD_CODE_BYTES {
            return Err("code exceeds harness limit".into());
        }
        let hash = code_hash(&code);
        if hex(&hash) != record.code_hash {
            return Err("stored code hash mismatch".into());
        }
        let program = parse(&code).map_err(|e| e.to_string())?;
        let mut storage = Storage::from_persisted_bytes(&decode(&record.storage)?, &program)
            .map_err(|e| e.to_string())?;
        if !storage.staged_map_entries().is_empty() {
            return Err("v2 contract record contains inline map entries".into());
        }
        storage.attach_map_reader(Rc::new(CallbackMapReader {
            read: self.parent.read,
            context: self.parent.context,
            address: *address,
            read_count: self.parent.read_count.clone(),
            read_bytes: self.parent.read_bytes.clone(),
            staged: self.map_writes.clone(),
        }));
        let entrypoints = program
            .functions
            .iter()
            .map(|f| (function_selector(f), f.name.clone()))
            .collect();
        Ok(Some(DeployedContract {
            bytecode: code,
            code_hash: hash,
            storage,
            entrypoints,
        }))
    }
    fn store_contract(
        &mut self,
        address: Address,
        contract: DeployedContract,
    ) -> Result<(), String> {
        let bytes = serde_json::to_vec(&Stored {
            version: 2,
            bytecode: hex(&contract.bytecode),
            code_hash: hex(&contract.code_hash),
            storage: hex(&contract
                .storage
                .to_persisted_scalar_bytes()
                .map_err(|e| e.to_string())?),
        })
        .map_err(|e| e.to_string())?;
        let mut proposed = self.writes.clone();
        proposed.insert(key(&address), bytes);
        let mut map_updates = BTreeMap::new();
        for (field, keys, word) in contract.storage.staged_map_entries() {
            let name = map_key(&address, &field, &keys)?;
            let value = if word == [0; 32] {
                Vec::new()
            } else {
                word.to_vec()
            };
            proposed.insert(name.clone(), value.clone());
            map_updates.insert(name, value);
        }
        let total = proposed.values().map(Vec::len).sum::<usize>();
        if proposed.len() > MAX_WRITES
            || proposed.values().any(|value| value.len() > MAX_BYTES)
            || total > MAX_BYTES / 3
        {
            return Err("state write exceeds harness limit".into());
        }
        self.writes = proposed;
        self.map_writes.borrow_mut().extend(map_updates);
        Ok(())
    }
    fn load_balance(&self, _: &Address) -> Result<[u8; 32], String> {
        Ok([0; 32])
    }
    fn store_balance(&mut self, _: Address, balance: [u8; 32]) -> Result<(), String> {
        if balance != [0; 32] {
            return Err("payable operations disabled in chain harness".into());
        }
        Ok(())
    }
    fn commit(self) -> Result<(), String> {
        self.parent.committed = self.writes;
        Ok(())
    }
}

fn failure(f: HostFailure) -> Json {
    json!({"success": false, "gasUsed": f.gas_used.to_string(), "kind": format!("{:?}", f.kind), "message": f.message, "writes": [], "deployments": [], "events": []})
}
fn rejected(kind: HostFailureKind, message: String, contract: Address) -> Result<Vec<u8>, String> {
    serde_json::to_vec(&failure(HostFailure {
        kind,
        message,
        gas_used: 0,
        failed_contract: contract,
    }))
    .map_err(|e| e.to_string())
}
fn execute(input: &[u8], read: ReadFn, context: usize) -> Result<Vec<u8>, String> {
    let request: Request = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    if request.version != 1 || request.chain_id == 0 || request.gas_limit > 10_000_000 {
        return Err("unsupported request version/context/limit".into());
    }
    let caller = address(&request.caller)?;
    let arguments = values::decode(&decode(&request.arguments)?).map_err(|e| e.to_string())?;
    let mut host = TransactionalHost::new(CallbackState {
        read,
        context,
        committed: BTreeMap::new(),
        read_bytes: Rc::new(Cell::new(0)),
        read_count: Rc::new(Cell::new(0)),
    });
    let (result, gas, deployments, events) = match request.operation.as_str() {
        "deploy" => {
            if request.contract.is_some() {
                return Err("unexpected contract in deploy".into());
            }
            let bytecode = decode(&request.bytecode.ok_or("missing bytecode")?)?;
            if bytecode.len() > MAX_CHILD_CODE_BYTES {
                return Err("code exceeds harness limit".into());
            }
            let entrypoints = if request.selector.is_some() {
                let program = match parse(&bytecode) {
                    Ok(program) => program,
                    Err(error) => {
                        return rejected(
                            HostFailureKind::InvalidBytecode,
                            error.to_string(),
                            caller,
                        )
                    }
                };
                program
                    .functions
                    .iter()
                    .map(|f| (function_selector(f), f.name.clone()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let initializer =
                match selected_function(request.function, request.selector, entrypoints) {
                    Ok(function) => function,
                    Err(message) => {
                        return rejected(HostFailureKind::InvalidEntrypoint, message, caller)
                    }
                };
            if initializer.is_none() && !arguments.is_empty() {
                return Err("arguments require initializer".into());
            }
            match host.deploy_values(DeployRequest {
                deployer: caller,
                nonce: request.nonce,
                bytecode,
                initializer: initializer.map(|function| Initializer {
                    function,
                    arguments,
                }),
                value: [0; 32],
                gas_limit: request.gas_limit,
                chain_id: request.chain_id,
                block_height: request.block_height,
                block_timestamp: request.block_timestamp,
            }) {
                DeployOutcome::Failure(f) => {
                    return serde_json::to_vec(&failure(f)).map_err(|e| e.to_string())
                }
                DeployOutcome::Success(r) => (
                    Value::Word(lithovm_bytecode::ValueType::Address, r.contract),
                    r.gas_used,
                    r.deployments,
                    r.events,
                ),
            }
        }
        "call" => {
            if request.bytecode.is_some() {
                return Err("unexpected bytecode in call".into());
            }
            let contract = address(&request.contract.ok_or("missing contract")?)?;
            let entrypoints = if request.selector.is_some() {
                let transaction = host.state_mut().begin_transaction()?;
                match transaction.load_contract(&contract) {
                    Ok(Some(deployed)) => deployed.entrypoints,
                    Ok(None) => {
                        return rejected(
                            HostFailureKind::MissingContract,
                            "missing contract".into(),
                            contract,
                        )
                    }
                    Err(message) => return rejected(HostFailureKind::State, message, contract),
                }
            } else {
                BTreeMap::new()
            };
            let function = match selected_function(request.function, request.selector, entrypoints)
            {
                Ok(Some(function)) => function,
                Ok(None) => {
                    return rejected(
                        HostFailureKind::UnknownSelector,
                        "call requires a native selector".into(),
                        contract,
                    )
                }
                Err(message) => {
                    return rejected(HostFailureKind::UnknownSelector, message, contract)
                }
            };
            match host.execute_values(CallRequest {
                contract,
                function,
                arguments,
                caller,
                value: [0; 32],
                gas_limit: request.gas_limit,
                chain_id: request.chain_id,
                block_height: request.block_height,
                block_timestamp: request.block_timestamp,
            }) {
                HostOutcome::Failure(f) => {
                    return serde_json::to_vec(&failure(f)).map_err(|e| e.to_string())
                }
                HostOutcome::Success(r) => {
                    (r.result.return_value, r.gas_used, r.deployments, r.events)
                }
            }
        }
        _ => return Err("unknown operation".into()),
    };
    let writes: Vec<_> = host
        .state()
        .committed
        .iter()
        .map(|(key, value)| {
            if value.is_empty() {
                json!({"key":key, "delete":true})
            } else {
                json!({"key":key, "value":hex(value)})
            }
        })
        .collect();
    let deployments: Vec<_> = deployments.iter().map(|r| {
        let origin = match r.origin { DeploymentOrigin::Transaction { nonce } => json!({"kind":"transaction", "nonce":nonce.to_string()}), DeploymentOrigin::Child { template, salt } => json!({"kind":"child", "template":hex(&template[12..]), "salt":hex(&salt)}) };
        json!({"contract":hex(&r.contract[12..]), "creator":hex(&r.creator[12..]), "codeHash":hex(&r.code_hash), "origin":origin})
    }).collect();
    let events = events.iter().map(|e| {
        let values: Vec<_> = e.event.fields.iter().map(|(_, _, v)| v.clone()).collect();
        Ok(json!({"contract":hex(&e.contract[12..]), "name":e.event.name, "values":hex(&values::encode(&values).map_err(|e| e.to_string())?)}))
    }).collect::<Result<Vec<_>, String>>()?;
    let output = serde_json::to_vec(&json!({"success":true,"gasUsed":gas.to_string(),"result":hex(&values::encode(&[result]).map_err(|e| e.to_string())?),"writes":writes,"deployments":deployments,"events":events})).map_err(|e| e.to_string())?;
    if output.len() > MAX_BYTES {
        return Err("output exceeds harness limit".into());
    }
    Ok(output)
}

/// # Safety
/// Input/output pointers and callback must be valid for the call; callback must
/// not unwind. Use only the paired free function for the returned buffer, once.
#[no_mangle]
pub unsafe extern "C" fn lithovm_execute_v1(
    input: *const u8,
    len: usize,
    context: usize,
    read: Option<ReadFn>,
    output: *mut Buffer,
) -> i32 {
    execute_abi(input, len, context, read, None, output)
}

/// # Safety
/// The callbacks and buffers must be valid for the synchronous call. Fuel
/// charges are immediate and must not unwind or re-enter the same invocation.
#[no_mangle]
pub unsafe extern "C" fn lithovm_execute_v2(
    input: *const u8,
    len: usize,
    context: usize,
    read: Option<ReadFn>,
    charge: Option<ChargeFn>,
    output: *mut Buffer,
) -> i32 {
    if charge.is_none() {
        if !output.is_null() {
            unsafe {
                *output = Buffer {
                    data: std::ptr::null_mut(),
                    len: 0,
                };
            }
        }
        return 1;
    }
    execute_abi(input, len, context, read, charge, output)
}

fn execute_abi(
    input: *const u8,
    len: usize,
    context: usize,
    read: Option<ReadFn>,
    charge: Option<ChargeFn>,
    output: *mut Buffer,
) -> i32 {
    if output.is_null() {
        return 1;
    }
    unsafe {
        *output = Buffer {
            data: std::ptr::null_mut(),
            len: 0,
        };
    }
    if input.is_null() || len == 0 || len > MAX_BYTES || read.is_none() {
        return 1;
    }
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let run = || {
            execute(
                unsafe { std::slice::from_raw_parts(input, len) },
                read.unwrap(),
                context,
            )
        };
        match charge {
            Some(callback) => unsafe { lithovm::with_external_fuel(context, callback, run) },
            None => run(),
        }
    }));
    match outcome {
        Ok(Ok(bytes)) => {
            let mut boxed = bytes.into_boxed_slice();
            unsafe {
                *output = Buffer {
                    data: boxed.as_mut_ptr(),
                    len: boxed.len(),
                };
            }
            std::mem::forget(boxed);
            0
        }
        Ok(Err(_)) => 2,
        Err(_) => 3,
    }
}

/// # Safety
/// Must be an unmodified buffer returned by lithovm_execute_v1, not already freed.
#[no_mangle]
pub unsafe extern "C" fn lithovm_free_v1(buffer: Buffer) {
    if !buffer.data.is_null() {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                buffer.data,
                buffer.len,
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state() -> CallbackState {
        CallbackState {
            read: absent,
            context: 0,
            committed: BTreeMap::new(),
            read_bytes: Rc::new(Cell::new(0)),
            read_count: Rc::new(Cell::new(0)),
        }
    }

    fn contract() -> DeployedContract {
        let artifact =
            lithic_lithovm::compile("contract C { pub fn value() -> u64 { return 42; } }").unwrap();
        let bytecode = decode(&artifact.bytecode).unwrap();
        DeployedContract {
            code_hash: code_hash(&bytecode),
            bytecode,
            storage: Storage::default(),
            entrypoints: BTreeMap::new(),
        }
    }

    #[test]
    fn stored_hash_guard_rejects_well_formed_record() {
        let mut state = state();
        let mut tx = state.begin_transaction().unwrap();
        let address = [1; 32];
        tx.store_contract(address, contract()).unwrap();
        assert!(tx.load_contract(&address).unwrap().is_some());
        let mut record: Stored = serde_json::from_slice(&tx.writes[&key(&address)]).unwrap();
        record.code_hash = hex(&[255; 32]);
        tx.writes
            .insert(key(&address), serde_json::to_vec(&record).unwrap());
        assert_eq!(
            tx.load_contract(&address).unwrap_err(),
            "stored code hash mismatch"
        );
    }

    #[test]
    fn legacy_inline_record_requires_explicit_migration() {
        let mut state = state();
        let mut tx = state.begin_transaction().unwrap();
        let address = [1; 32];
        tx.store_contract(address, contract()).unwrap();
        let mut record: Stored = serde_json::from_slice(&tx.writes[&key(&address)]).unwrap();
        record.version = 1;
        tx.writes
            .insert(key(&address), serde_json::to_vec(&record).unwrap());
        assert_eq!(
            tx.load_contract(&address).unwrap_err(),
            "unsupported contract record; explicit migration required"
        );
    }

    #[test]
    fn payable_guard_rejects_nonzero_balance_without_commit() {
        let mut state = state();
        let mut tx = state.begin_transaction().unwrap();
        tx.store_balance([1; 32], [0; 32]).unwrap();
        let mut balance = [0; 32];
        balance[31] = 1;
        assert_eq!(
            tx.store_balance([1; 32], balance).unwrap_err(),
            "payable operations disabled in chain harness"
        );
        assert!(tx.writes.is_empty());
    }

    #[test]
    fn read_count_guard_rejects_257th_read() {
        let state = state();
        for _ in 0..256 {
            assert_eq!(state.read(&key(&[1; 32])).unwrap(), None);
        }
        assert_eq!(
            state.read(&key(&[1; 32])).unwrap_err(),
            "state read count exceeds harness limit"
        );
    }

    #[test]
    fn aggregate_write_guard_accepts_limit_rejects_one_byte_over() {
        let mut state = state();
        let mut tx = state.begin_transaction().unwrap();
        let address = [1; 32];
        tx.store_contract(address, contract()).unwrap();
        let record_size = tx.writes[&key(&address)].len();
        // Model the serialized bytes already staged by earlier contract writes.
        // Two records keep the independent record-count limit out of this test.
        tx.writes
            .insert(key(&[2; 32]), vec![0; MAX_BYTES / 3 - record_size]);
        tx.store_contract(address, contract()).unwrap();
        tx.writes.get_mut(&key(&[2; 32])).unwrap().push(0);
        let before = tx.writes.clone();
        assert_eq!(
            tx.store_contract(address, contract()).unwrap_err(),
            "state write exceeds harness limit"
        );
        assert_eq!(tx.writes, before);
    }
    unsafe extern "C" fn absent(_: usize, _: *const u8, _: usize, _: *mut u8, _: usize) -> isize {
        -1
    }
    #[test]
    fn c_abi_rejects_bad_buffers_and_releases_success_output() {
        let mut output = Buffer {
            data: std::ptr::null_mut(),
            len: 0,
        };
        assert_eq!(
            unsafe { lithovm_execute_v1(std::ptr::null(), 1, 0, Some(absent), &mut output) },
            1
        );
        assert!(output.data.is_null());
        let bad = b"{}";
        assert_eq!(
            unsafe { lithovm_execute_v1(bad.as_ptr(), bad.len(), 0, Some(absent), &mut output) },
            2
        );
        assert!(output.data.is_null());
        let artifact =
            lithic_lithovm::compile("contract C { pub fn value() -> u64 { return 42; } }").unwrap();
        let request = serde_json::to_vec(&json!({"version":1,"operation":"deploy","caller":"0x0000000000000000000000000000000000000009","bytecode":artifact.bytecode,"arguments":"0x4c56414c010000","nonce":1,"gas_limit":100000,"chain_id":700777,"block_height":1,"block_timestamp":2})).unwrap();
        for _ in 0..100 {
            assert_eq!(
                unsafe {
                    lithovm_execute_v1(
                        request.as_ptr(),
                        request.len(),
                        0,
                        Some(absent),
                        &mut output,
                    )
                },
                0
            );
            let response: Json = serde_json::from_slice(unsafe {
                std::slice::from_raw_parts(output.data, output.len)
            })
            .unwrap();
            assert_eq!(response["success"], true);
            assert_eq!(response["writes"].as_array().unwrap().len(), 1);
            unsafe {
                lithovm_free_v1(output);
            }
            output = Buffer {
                data: std::ptr::null_mut(),
                len: 0,
            };
        }
    }
}
