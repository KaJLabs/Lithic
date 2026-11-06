//! Offline native gateway preparation; deliberately has no RPC or signer.
use lithic_lithovm::{compile, Artifact, EntrypointArtifact};
use lithovm_bytecode::{values, values::Value, ValueType};
use serde::Deserialize;
use serde_json::{json, Value as Json};
use sha3::{Digest, Keccak256};
use std::{collections::BTreeMap, io::Read};

const HELP: &str = "lithdev — native gateway preparation (Makalu 700777)

USAGE:
  lithdev prepare-deploy FILE.lithic --chain-id 700777 [--initializer NAME --args ARGS.json]
  lithdev prepare-call FILE.lithic --chain-id 700777 --contract ADDRESS --function NAME [--args ARGS.json]
  lithdev decode-result 0xEVM_RETURN_BYTES

ARGS.json is an array of {\"type\":\"u256\",\"value\":\"42\"} objects.
Supported types: u64/u256 (decimal strings), bool, address, bytes32, string.
If the source declares initialize, prepare-deploy requires --initializer initialize.
Output is an unsigned intent; confirm live runtime/code, simulate and bind all
transaction/authority fields in the wallet workflow before signing. This command
does not access wallets, broadcast, estimate gas or claim deployment/finality.";
const GATEWAY: &str = "0x000000000000000000000000000000000000f000";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Argument {
    #[serde(rename = "type")]
    kind: String,
    value: Json,
}

fn read_file(path: &str) -> Result<String, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("input exceeds 4 MiB".into());
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn unhex(value: &str, length: Option<usize>) -> Result<Vec<u8>, String> {
    let raw = value.strip_prefix("0x").ok_or("hex prefix required")?;
    let bytes = hex::decode(raw).map_err(|e| e.to_string())?;
    if length.is_some_and(|n| bytes.len() != n) {
        return Err("incorrect hex byte length".into());
    }
    Ok(bytes)
}

fn integer(value: &str) -> Result<[u8; 32], String> {
    if value.is_empty()
        || value.len() > 78
        || value.len() > 1 && value.starts_with('0')
        || !value.bytes().all(|c| c.is_ascii_digit())
    {
        return Err("integer must be a canonical unsigned decimal string".into());
    }
    let mut word = [0u8; 32];
    for digit in value.bytes() {
        let mut carry = u16::from(digit - b'0');
        for byte in word.iter_mut().rev() {
            let next = u16::from(*byte) * 10 + carry;
            *byte = next as u8;
            carry = next >> 8;
        }
        if carry != 0 {
            return Err("integer exceeds uint256".into());
        }
    }
    Ok(word)
}

fn decimal(mut word: [u8; 32]) -> String {
    let mut digits = Vec::new();
    loop {
        let mut remainder = 0u16;
        for byte in &mut word {
            let next = remainder * 256 + u16::from(*byte);
            *byte = (next / 10) as u8;
            remainder = next % 10;
        }
        digits.push(b'0' + remainder as u8);
        if word == [0; 32] {
            break;
        }
    }
    digits.reverse();
    String::from_utf8(digits).expect("decimal digits are ASCII")
}

fn argument(value: &Argument) -> Result<Value, String> {
    let ty = value.kind.as_str();
    let input = &value.value;
    if ty == "string" {
        return Ok(Value::String(
            input.as_str().ok_or("string value required")?.into(),
        ));
    }
    if ty == "bytes" {
        let data = unhex(input.as_str().ok_or("hex string required")?, None)?;
        if data.len() > values::MAX_BYTES_LENGTH {
            return Err("bytes exceeds ABI byte limit".into());
        }
        return Ok(Value::Bytes(data));
    }
    let (kind, word) = match ty {
        "u64" | "u256" => (
            if ty == "u64" {
                ValueType::U64
            } else {
                ValueType::U256
            },
            integer(
                input
                    .as_str()
                    .ok_or("decimal string required; JSON numbers are rejected")?,
            )?,
        ),
        "bool" => {
            let mut word = [0; 32];
            word[31] = u8::from(input.as_bool().ok_or("boolean value required")?);
            (ValueType::Bool, word)
        }
        "address" | "bytes32" => {
            let bytes = unhex(
                input.as_str().ok_or("hex string required")?,
                Some(if ty == "address" { 20 } else { 32 }),
            )?;
            let mut word = [0; 32];
            word[32 - bytes.len()..].copy_from_slice(&bytes);
            (
                if ty == "address" {
                    ValueType::Address
                } else {
                    ValueType::Bytes32
                },
                word,
            )
        }
        _ => return Err("unsupported argument type".into()),
    };
    lithovm_bytecode::validate_word(kind, &word).map_err(|e| e.to_string())?;
    Ok(Value::Word(kind, word))
}

fn encode_arguments(entry: &EntrypointArtifact, args: &[Argument]) -> Result<Vec<u8>, String> {
    let types = entry
        .signature
        .split_once('(')
        .ok_or("invalid compiled signature")?
        .1;
    let types = types
        .strip_suffix(')')
        .ok_or("invalid compiled signature")?;
    let types: Vec<_> = if types.is_empty() {
        vec![]
    } else {
        types.split(',').collect()
    };
    if types.len() != args.len() || types.iter().zip(args).any(|(ty, arg)| arg.kind != *ty) {
        return Err("argument types/count differ from compiled function".into());
    }
    let values = args.iter().map(argument).collect::<Result<Vec<_>, _>>()?;
    values::encode(&values).map_err(|e| e.to_string())
}

fn word(value: usize) -> [u8; 32] {
    let mut bytes = [0; 32];
    bytes[24..].copy_from_slice(&(value as u64).to_be_bytes());
    bytes
}

fn dynamic(bytes: &[u8]) -> Vec<u8> {
    let mut output = word(bytes.len()).to_vec();
    output.extend(bytes);
    output.resize(32 + bytes.len().div_ceil(32) * 32, 0);
    output
}

fn gateway(kind: &str, target: &[u8], selector: &[u8], args: &[u8]) -> Vec<u8> {
    let deploy = kind == "deploy";
    let method = if deploy {
        "deploy(bytes,bytes32,bytes)"
    } else {
        "call(address,bytes32,bytes)"
    };
    let mut output = Keccak256::digest(method.as_bytes())[..4].to_vec();
    let code = dynamic(target);
    if deploy {
        output.extend(word(96));
    } else {
        output.extend([0u8; 12]);
        output.extend(target);
    }
    output.extend(selector);
    output.extend(word(96 + if deploy { code.len() } else { 0 }));
    if deploy {
        output.extend(code);
    }
    output.extend(dynamic(args));
    output
}

fn select<'a>(artifact: &'a Artifact, name: &str) -> Result<&'a EntrypointArtifact, String> {
    artifact
        .entrypoints
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| "function absent from compiled entrypoints".into())
}

fn prepare(command: &str, source: &str, flags: &BTreeMap<String, String>) -> Result<Json, String> {
    if flags.get("--chain-id").map(String::as_str) != Some("700777") {
        return Err("explicit --chain-id 700777 required for the documented Makalu profile".into());
    }
    let artifact = compile(source).map_err(|e| e.to_string())?;
    let program =
        lithovm_bytecode::parse(&unhex(&artifact.bytecode, None)?).map_err(|e| e.to_string())?;
    if program.bytecode_version() > lithovm_bytecode::CREATE_VERSION {
        return Err(
            "source requires a newer native runtime than the documented Makalu v14 profile".into(),
        );
    }
    let args: Vec<Argument> = match flags.get("--args") {
        Some(path) => serde_json::from_str(&read_file(path)?).map_err(|e| e.to_string())?,
        None => vec![],
    };
    if args.len() > values::MAX_VALUES {
        return Err("too many arguments".into());
    }
    let (kind, target, entry) = if command == "prepare-deploy" {
        let code = unhex(&artifact.bytecode, None)?;
        if code.is_empty() || code.len() > 65536 {
            return Err("native bytecode must be 1..65536 bytes; admission is not a deployability guarantee".into());
        }
        let name = flags.get("--initializer");
        if artifact.entrypoints.iter().any(|f| f.name == "initialize")
            && name.map(String::as_str) != Some("initialize")
        {
            return Err(
                "source declares initialize; explicit atomic --initializer initialize required"
                    .into(),
            );
        }
        (
            "deploy",
            code,
            name.map(|n| select(&artifact, n)).transpose()?,
        )
    } else {
        let target = unhex(
            flags.get("--contract").ok_or("--contract required")?,
            Some(20),
        )?;
        if target == [0u8; 20] {
            return Err("nonzero native target required".into());
        }
        let entry = select(
            &artifact,
            flags.get("--function").ok_or("--function required")?,
        )?;
        ("call", target, Some(entry))
    };
    let (selector, encoded) = match entry {
        Some(entry) => (
            unhex(&entry.selector, Some(32))?,
            encode_arguments(entry, &args)?,
        ),
        None if args.is_empty() => (vec![0; 32], vec![]),
        None => return Err("arguments require an initializer".into()),
    };
    let calldata = gateway(kind, &target, &selector, &encoded);
    Ok(
        json!({"schemaVersion":"lithdev-native-intent-v1", "operation":kind,
        "chainId":700777, "to":GATEWAY, "value":"0", "data":format!("0x{}",hex::encode(calldata)),
        "nativeTarget":if kind=="call" {Some(format!("0x{}",hex::encode(target)))} else {None},
        "nativeSignature":entry.map(|f|&f.signature), "expectedCodeHash":artifact.code_hash,
        "artifact":artifact, "executionAuthorized":false, "deploymentStatus":"not-submitted",
        "transactionFieldsPending":["from","nonce","gas","maxFeePerGas","maxPriorityFeePerGas"],
        "requiredChecks":["approved operation and live runtime profile","fresh chain/account/code evidence",
            "simulation and gas limits including native-envelope failure behavior","wallet signing and exact transaction verification"]}),
    )
}

fn decode_result(encoded: &str) -> Result<Json, String> {
    if encoded.len() > 2 + (64 + values::MAX_ENVELOPE_BYTES) * 2 {
        return Err("gateway result exceeds bound".into());
    }
    let raw = unhex(encoded, None)?;
    if raw.len() < 64 || raw[..32] != word(32) || raw[32..56] != [0; 24] {
        return Err("noncanonical gateway result header".into());
    }
    let length = u64::from_be_bytes(raw[56..64].try_into().expect("header length checked"));
    if length > values::MAX_ENVELOPE_BYTES as u64 {
        return Err("native result exceeds envelope bound".into());
    }
    let length = length as usize;
    if raw.len() != 64 + length.div_ceil(32) * 32 || raw[64 + length..].iter().any(|b| *b != 0) {
        return Err("noncanonical gateway result length/padding".into());
    }
    let decoded = values::decode(&raw[64..64 + length]).map_err(|e| e.to_string())?;
    let result: Vec<_> = decoded
        .iter()
        .map(|value| match value {
            Value::String(s) => json!({"type":"string","value":s}),
            Value::Bytes(data) => json!({"type":"bytes","value":format!("0x{}",hex::encode(data))}),
            Value::Word(ty, w) => match ty {
                ValueType::U64 => json!({"type":"u64","value":decimal(*w)}),
                ValueType::U256 => json!({"type":"u256","value":decimal(*w)}),
                ValueType::Bool => json!({"type":"bool","value":w[31]==1}),
                ValueType::Address => {
                    json!({"type":"address","value":format!("0x{}",hex::encode(&w[12..]))})
                }
                ValueType::Bytes32 => {
                    json!({"type":"bytes32","value":format!("0x{}",hex::encode(w))})
                }
                ValueType::String | ValueType::Bytes => {
                    unreachable!("dynamic values have separate native variants")
                }
            },
        })
        .collect();
    Ok(json!({"values":result,"canonicalProofVerified":false}))
}

fn run(args: &[String]) -> Result<Json, String> {
    let command = args.first().ok_or("command required; use --help")?;
    if command == "decode-result" {
        if args.len() != 2 {
            return Err("decode-result requires exactly one hex result".into());
        }
        return decode_result(&args[1]);
    }
    if !matches!(command.as_str(), "prepare-deploy" | "prepare-call") || args.len() < 4 {
        return Err("invalid command; use --help".into());
    }
    let allowed = if command == "prepare-deploy" {
        vec!["--chain-id", "--initializer", "--args"]
    } else {
        vec!["--chain-id", "--contract", "--function", "--args"]
    };
    let mut flags = BTreeMap::new();
    let mut items = args[2..].chunks_exact(2);
    for pair in &mut items {
        if !allowed.contains(&pair[0].as_str())
            || flags.insert(pair[0].clone(), pair[1].clone()).is_some()
        {
            return Err("unknown or duplicate option".into());
        }
    }
    if !items.remainder().is_empty() {
        return Err("option value required".into());
    }
    prepare(command, &read_file(&args[1])?, &flags)
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() == 1 && matches!(args[0].as_str(), "--help" | "-h") {
        println!("{HELP}");
        return;
    }
    match run(&args) {
        Ok(output) => println!(
            "{}",
            serde_json::to_string_pretty(&output).expect("JSON output")
        ),
        Err(error) => {
            eprintln!("lithdev: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "contract Tool { pub fn answer() -> u64 { return 42; } }";
    const MAX: &str =
        "115792089237316195423570985008687907853269984665640564039457584007913129639935";

    fn arg(kind: &str, value: Json) -> Argument {
        Argument {
            kind: kind.into(),
            value,
        }
    }

    #[test]
    fn calldata_matches_independent_javascript_gateway_vectors() {
        let fixture: Json = serde_json::from_str(include_str!(
            "../../../../sdk/tests/native-gateway-vectors.json"
        ))
        .unwrap();
        assert_eq!(fixture["cases"][1]["arguments"][5]["value"], "é\0native");
        for case in fixture["cases"].as_array().unwrap() {
            let args = if case["arguments"].is_null() {
                vec![]
            } else {
                let inputs: Vec<Argument> =
                    serde_json::from_value(case["arguments"].clone()).unwrap();
                let values = inputs
                    .iter()
                    .map(argument)
                    .collect::<Result<Vec<_>, _>>()
                    .unwrap();
                values::encode(&values).unwrap()
            };
            assert_eq!(
                format!("0x{}", hex::encode(&args)),
                case["encodedArguments"]
            );
            let actual = gateway(
                case["operation"].as_str().unwrap(),
                &unhex(case["target"].as_str().unwrap(), None).unwrap(),
                &unhex(case["selector"].as_str().unwrap(), Some(32)).unwrap(),
                &args,
            );
            assert_eq!(format!("0x{}", hex::encode(actual)), case["calldata"]);
        }
    }

    #[test]
    fn integer_precision_and_boundary_failures() {
        assert_eq!(integer(MAX).unwrap(), [255; 32]);
        assert_eq!(decimal([255; 32]), MAX);
        assert_eq!(decimal(integer("0").unwrap()), "0");
        assert_eq!(
            decimal(integer("9007199254740993").unwrap()),
            "9007199254740993"
        );
        for value in [
            "",
            "00",
            "01",
            "-1",
            "+1",
            "1e3",
            " 1",
            "1.0",
            "115792089237316195423570985008687907853269984665640564039457584007913129639936",
        ] {
            assert!(integer(value).is_err(), "{value}");
        }
        assert!(argument(&arg("u64", json!("18446744073709551616"))).is_err());
        assert!(argument(&arg("u256", json!(42))).is_err());
        assert!(argument(&arg("bool", json!(1))).is_err());
    }

    #[test]
    fn strict_argument_json_and_utf8_byte_bounds() {
        for raw in [
            r#"{"type":"u64","value":"1","extra":true}"#,
            r#"{"type":"u64","type":"u256","value":"1"}"#,
            r#"{"type":"u64","value":"1","value":"2"}"#,
        ] {
            assert!(serde_json::from_str::<Argument>(raw).is_err());
        }
        let entry = EntrypointArtifact {
            name: "text".into(),
            signature: "text(string)".into(),
            selector: String::new(),
        };
        assert!(encode_arguments(&entry, &[arg("string", json!("é".repeat(2048)))]).is_ok());
        assert!(encode_arguments(&entry, &[arg("string", json!("é".repeat(2049)))]).is_err());
        assert!(encode_arguments(&entry, &[arg("u64", json!("1"))]).is_err());
        assert!(argument(&arg("address", json!("0x1234"))).is_err());
        assert!(argument(&arg("bytes32", json!("0x1234"))).is_err());
    }

    #[test]
    fn deploy_preparation_preserves_gateway_offsets_and_empty_initializer() {
        let flags = BTreeMap::from([("--chain-id".into(), "700777".into())]);
        let result = prepare("prepare-deploy", SOURCE, &flags).unwrap();
        assert_eq!(result["executionAuthorized"], false);
        assert_eq!(result["deploymentStatus"], "not-submitted");
        assert_eq!(result["to"], GATEWAY);
        let data = unhex(result["data"].as_str().unwrap(), None).unwrap();
        let code = unhex(result["artifact"]["bytecode"].as_str().unwrap(), None).unwrap();
        assert_eq!(
            &data[..4],
            &Keccak256::digest(b"deploy(bytes,bytes32,bytes)")[..4]
        );
        assert_eq!(&data[4..36], &word(96));
        assert_eq!(&data[36..68], &[0; 32]);
        assert_eq!(&data[68..100], &word(96 + dynamic(&code).len()));
        assert_eq!(&data[100..100 + dynamic(&code).len()], dynamic(&code));
        assert_eq!(&data[data.len() - 32..], &[0; 32]);
    }

    #[test]
    fn atomic_initialize_is_required_by_source_and_encoded_explicitly() {
        let source="contract Tool { state { ready: bool; } pub fn initialize() -> bool { require(self.ready == false); self.ready = true; return true; } }";
        let mut flags = BTreeMap::from([("--chain-id".into(), "700777".into())]);
        assert!(prepare("prepare-deploy", source, &flags)
            .unwrap_err()
            .contains("atomic"));
        flags.insert("--initializer".into(), "initialize".into());
        let result = prepare("prepare-deploy", source, &flags).unwrap();
        assert_eq!(result["nativeSignature"], "initialize()");
        let data = unhex(result["data"].as_str().unwrap(), None).unwrap();
        assert_ne!(&data[36..68], &[0; 32]);
        assert_eq!(&data[data.len() - 32..data.len() - 25], b"LVAL\x01\x00\x00");
    }

    #[test]
    fn call_uses_full_native_selector_and_documented_evm_method() {
        let flags = BTreeMap::from([
            ("--chain-id".into(), "700777".into()),
            (
                "--contract".into(),
                "0x1111111111111111111111111111111111111111".into(),
            ),
            ("--function".into(), "answer".into()),
        ]);
        let result = prepare("prepare-call", SOURCE, &flags).unwrap();
        let data = unhex(result["data"].as_str().unwrap(), None).unwrap();
        assert_eq!(hex::encode(&data[..4]), "cf511cf7");
        assert_eq!(&data[4..16], &[0; 12]);
        assert_eq!(&data[16..36], &[0x11; 20]);
        assert_eq!(&data[36..68], &Keccak256::digest(b"answer()")[..]);
        assert_eq!(&data[68..100], &word(96));
        assert_eq!(data.len(), 164);
    }

    #[test]
    fn result_decoding_roundtrips_all_types_without_precision_loss() {
        let values = vec![
            Value::Word(ValueType::U64, integer("18446744073709551615").unwrap()),
            Value::Word(ValueType::U256, [255; 32]),
            Value::Word(ValueType::Bool, integer("1").unwrap()),
            Value::Word(ValueType::Address, integer("1").unwrap()),
            Value::Word(ValueType::Bytes32, [255; 32]),
            Value::String("é\0native".into()),
        ];
        let mut encoded = word(32).to_vec();
        encoded.extend(dynamic(&values::encode(&values).unwrap()));
        let result = decode_result(&format!("0x{}", hex::encode(&encoded))).unwrap();
        assert_eq!(result["values"][0]["value"], "18446744073709551615");
        assert_eq!(result["values"][1]["value"], MAX);
        assert_eq!(result["values"][2]["value"], true);
        assert_eq!(result["values"][5]["value"], "é\0native");
        for mutation in 0..4 {
            let mut bad = encoded.clone();
            match mutation {
                0 => bad[31] = 64,
                1 => bad.push(0),
                2 => {
                    bad.pop();
                }
                _ => *bad.last_mut().unwrap() = 1,
            };
            assert!(decode_result(&format!("0x{}", hex::encode(bad))).is_err());
        }
    }

    #[test]
    fn unsupported_chain_function_and_options_fail_closed() {
        for chain in ["9005", "0", "0700777"] {
            let flags = BTreeMap::from([("--chain-id".into(), chain.into())]);
            assert!(prepare("prepare-deploy", SOURCE, &flags).is_err());
        }
        for options in [
            ["--rpc", "https://example.invalid"],
            ["--chain-id", "700777"],
        ] {
            let args = vec![
                "prepare-deploy".into(),
                "missing-file".into(),
                options[0].into(),
                options[1].into(),
                "--chain-id".into(),
                "700777".into(),
            ];
            assert!(run(&args).unwrap_err().contains("option"));
        }
    }

    #[test]
    fn binary_abi_is_strict_and_new_runtime_cannot_prepare_old_chain_intents() {
        let entry = EntrypointArtifact {
            name: "echo".into(),
            signature: "echo(bytes)".into(),
            selector: String::new(),
        };
        let encoded = encode_arguments(&entry, &[arg("bytes", json!("0x00ff80"))]).unwrap();
        assert_eq!(
            values::decode(&encoded).unwrap(),
            [Value::Bytes(vec![0, 255, 128])]
        );
        for invalid in [
            json!("0x0"),
            json!("0xgg"),
            json!("ff"),
            json!([0, 255]),
            json!(format!("0x{}", "ff".repeat(4097))),
        ] {
            assert!(encode_arguments(&entry, &[arg("bytes", invalid)]).is_err());
        }
        let mut result = word(32).to_vec();
        result.extend(dynamic(&encoded));
        let decoded = decode_result(&format!("0x{}", hex::encode(result))).unwrap();
        assert_eq!(
            decoded["values"][0],
            json!({"type":"bytes","value":"0x00ff80"})
        );
        let flags = BTreeMap::from([("--chain-id".into(), "700777".into())]);
        for (version, source) in [
            (15, "contract Binary { pub fn echo(data: bytes) -> bytes { return data; } }"),
            (17, "contract Compact { pub fn get(data: bytes) -> u64 { return bytes_get(data, 0); } }"),
        ] {
            assert_eq!(lithic_lithovm::compile(source).unwrap().bytecode_version,version);
            for command in ["prepare-deploy", "prepare-call"] {
                assert!(prepare(command, source, &flags)
                    .unwrap_err()
                    .contains("Makalu v14 profile"));
            }
        }
    }

    #[test]
    fn compiler_accepted_code_above_gateway_admission_is_rejected() {
        let mut source = String::from("contract Large {");
        for i in 0..900 {
            source.push_str(&format!(
                "pub fn f{i}{}() -> u64 {{ return 1; }}",
                "x".repeat(100)
            ));
        }
        source.push('}');
        let artifact = compile(&source).unwrap();
        assert!(unhex(&artifact.bytecode, None).unwrap().len() > 65536);
        let flags = BTreeMap::from([("--chain-id".into(), "700777".into())]);
        assert!(prepare("prepare-deploy", &source, &flags)
            .unwrap_err()
            .contains("1..65536"));
    }
}
