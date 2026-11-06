//! Local replay of the disabled keeper's candidate pricing over real FFI output.
//! This is neither a keeper execution nor approval of production gas economics.
use super::*;
use std::collections::BTreeSet;

const NATIVE_ENVELOPE: u64 = 10_000_000;

#[derive(Default)]
struct Meter {
    reads: u64,
    vm: u64,
    original_keys: BTreeSet<String>,
    records: BTreeMap<String, Vec<u8>>,
}

unsafe extern "C" fn read_state(
    ctx: usize,
    key: *const u8,
    len: usize,
    out: *mut u8,
    capacity: usize,
) -> isize {
    let meter = &mut *(ctx as *mut Meter);
    let key = std::str::from_utf8(std::slice::from_raw_parts(key, len)).unwrap();
    // Every physical Get costs 1,000 + 3 * (key + value bytes).
    let value = meter.records.get(key);
    meter.reads += 1_000 + 3 * (len + value.map_or(0, Vec::len)) as u64;
    meter.original_keys.insert(key.into());
    let Some(value) = value else {
        return -1;
    };
    if capacity < value.len() {
        return value.len() as isize;
    }
    std::ptr::copy_nonoverlapping(value.as_ptr(), out, value.len());
    value.len() as isize
}

unsafe extern "C" fn charge(ctx: usize, gas: u64) -> i32 {
    let meter = &mut *(ctx as *mut Meter);
    meter.vm += gas;
    i32::from(meter.vm + meter.reads <= NATIVE_ENVELOPE)
}

fn word(n: u64) -> [u8; 32] {
    let mut value = [0; 32];
    value[24..].copy_from_slice(&n.to_be_bytes());
    value
}

fn invoke(meter: &mut Meter, input: &[u8]) -> Json {
    let mut output = Buffer {
        data: std::ptr::null_mut(),
        len: 0,
    };
    assert_eq!(
        unsafe {
            lithovm_execute_v2(
                input.as_ptr(),
                input.len(),
                meter as *mut Meter as usize,
                Some(read_state),
                Some(charge),
                &mut output,
            )
        },
        0
    );
    let result =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(output.data, output.len) })
            .unwrap();
    unsafe { lithovm_free_v1(output) };
    result
}

#[test]
fn candidate_account_deployment_fits_native_envelope() {
    let artifact = lithic_lithovm::compile(include_str!(
        "../../../../sdk/contracts/standards/lep10015_native_account_candidate.lithic"
    ))
    .unwrap();
    let mut descriptors = word(32).to_vec();
    descriptors.extend(word(2));
    for signer in [
        "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf",
        "0x2b5ad5c4795c026514f8317c7a215e218dccd6cf",
    ] {
        descriptors.extend(address(signer).unwrap());
        descriptors.extend(word(1));
    }
    let arguments = values::encode(&[
        Value::Bytes(descriptors),
        Value::Word(lithovm_bytecode::ValueType::U64, word(2)),
    ])
    .unwrap();
    let input = serde_json::to_vec(&json!({
        "version":1,"operation":"deploy",
        "caller":"0x0000000000000000000000000000000000000009",
        "bytecode":artifact.bytecode,"function":"initialize",
        "arguments":hex(&arguments),"nonce":1,"gas_limit":NATIVE_ENVELOPE,
        "chain_id":700780,"block_height":1,"block_timestamp":100
    }))
    .unwrap();
    let mut meter = Meter::default();
    let result = invoke(&mut meter, &input);
    assert_eq!(result["success"], true, "{result}");
    assert_eq!(result["gasUsed"], meter.vm.to_string());
    assert_eq!(result["deployments"].as_array().unwrap().len(), 1);

    // Mirror statedb.go's changed-write and rounded key/value growth accounting.
    // There is no existing state and no refund; unchanged absent deletes cost 0.
    let (mut write_gas, mut growth_gas, mut stored_bytes) = (0, 0, 0);
    for write in result["writes"].as_array().unwrap() {
        let key = write["key"].as_str().unwrap();
        if !meter.original_keys.contains(key) {
            meter.reads += 1_000 + 3 * key.len() as u64;
        }
        if write["delete"] == true {
            continue;
        }
        let value = decode(write["value"].as_str().unwrap()).unwrap();
        let size = (key.len() + value.len()) as u64;
        stored_bytes += size;
        write_gas += 2_000 + 30 * size;
        growth_gas += 20_000 * size.div_ceil(32);
        meter.records.insert(key.into(), value);
    }
    let logs = serde_json::to_vec(&json!({
        "Deployments": result["deployments"], "Events": result["events"]
    }))
    .unwrap();
    let log_gas = 750 + 8 * logs.len() as u64;
    let total = meter.vm + meter.reads + write_gas + growth_gas + log_gas;
    eprintln!(
        "account deployment: code_bytes={} stored_key_value_bytes={stored_bytes} vm={} reads={} writes={write_gas} growth={growth_gas} logs={log_gas} native_total={total} envelope={NATIVE_ENVELOPE}",
        decode(&artifact.bytecode).unwrap().len(), meter.vm, meter.reads
    );
    // Gateway decode and outer Ethereum transaction costs are outside this cap.
    assert!(total <= NATIVE_ENVELOPE, "native account deployment requires {total} gas, exceeding the unchanged {NATIVE_ENVELOPE} envelope");

    // A second real C-ABI transaction must reload the binary record and its maps.
    let account = result["deployments"][0]["contract"].as_str().unwrap();
    let signer = address("0x7e5f4552091a69125d5dfcb7b8c2659029395bdf").unwrap();
    let args =
        values::encode(&[Value::Word(lithovm_bytecode::ValueType::Address, signer)]).unwrap();
    let call=serde_json::to_vec(&json!({"version":1,"operation":"call","caller":"0x0000000000000000000000000000000000000009","contract":account,"function":"signer_id","arguments":hex(&args),"nonce":2,"gas_limit":NATIVE_ENVELOPE,"chain_id":700780,"block_height":2,"block_timestamp":101})).unwrap();
    meter.vm = 0;
    meter.reads = 0;
    meter.original_keys.clear();
    let reloaded = invoke(&mut meter, &call);
    assert_eq!(reloaded["success"], true, "{reloaded}");
    let result = values::decode(&decode(reloaded["result"].as_str().unwrap()).unwrap()).unwrap();
    assert!(
        matches!(result.as_slice(), [Value::Word(lithovm_bytecode::ValueType::Bytes32, id)] if *id != [0;32])
    );
    for write in reloaded["writes"].as_array().unwrap() {
        assert_eq!(
            decode(write["value"].as_str().unwrap()).unwrap(),
            meter.records[write["key"].as_str().unwrap()],
            "view changed persisted record"
        );
    }
}
