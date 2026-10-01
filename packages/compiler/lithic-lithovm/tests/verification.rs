use lithic_lithovm::{
    compile,
    verification::{verify, ChainCode, Request, Source},
};

const ADDRESS: &str = "0x0000000000000000000000000000000000000007";
fn fixture() -> (Request, Vec<u8>) {
    let source = include_str!("../../../../sdk/contracts/standards/lax_lep100_v11.lithic");
    let artifact = compile(source).unwrap();
    let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
    (
        Request {
            schema_version: "lithovm-verification-v1".into(),
            chain_id: 700777,
            contract_address: ADDRESS.into(),
            code_hash: artifact.code_hash.clone(),
            sources: vec![Source {
                path: "LAX.lithic".into(),
                content: source.into(),
                keccak256: artifact.source_hash.clone(),
            }],
            artifact,
            transaction_hash: None,
        },
        bytes,
    )
}

#[test]
fn lax_rebuild_matches_independent_chain_observation() {
    let (request, bytes) = fixture();
    let result = verify(
        &request,
        ChainCode {
            chain_id: 700777,
            contract_address: ADDRESS,
            bytecode: &bytes,
        },
    )
    .unwrap();
    assert_eq!(
        result.rebuilt_code_hash,
        "0x4cb3b90d0744ff32e90ef4151fdc0f2a89927c67f659e12711114d9f4d2ce752"
    );
}

#[test]
fn rejects_tampered_metadata_source_and_identity() {
    for mutation in 0..8 {
        let (mut request, mut bytes) = fixture();
        match mutation {
            0 => request.sources[0].content.push(' '),
            1 => request.artifact.abi = serde_json::json!([]),
            2 => request.artifact.entrypoints.clear(),
            3 => request.chain_id = 9005,
            4 => request.sources[0].path = "../escape.lithic".into(),
            5 => request.artifact.compiler_version = "unknown".into(),
            6 => bytes[0] ^= 1,
            _ => request.sources.clear(),
        }
        assert!(
            verify(
                &request,
                ChainCode {
                    chain_id: 700777,
                    contract_address: ADDRESS,
                    bytecode: &bytes
                }
            )
            .is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn rejects_unknown_or_duplicate_request_fields() {
    let (request, _) = fixture();
    let mut value = serde_json::to_value(&request).unwrap();
    value["compilerCommand"] = "untrusted".into();
    assert!(serde_json::from_value::<Request>(value).is_err());
    let json = serde_json::to_string(&request).unwrap();
    let duplicate = json.replacen('{', "{\"chainId\":9005,", 1);
    assert!(serde_json::from_str::<Request>(&duplicate).is_err());
}
