use lithovm_bytecode::{
    values::{decode, encode, Value, MAX_ENVELOPE_BYTES, MAX_STRING_BYTES},
    ValueType,
};

#[test]
fn matches_independent_utf8_wire_vector() {
    // LVAL, version 1, two values: string "Aé", string "".
    let bytes = hex::decode("4c56414c01000206000341c3a9060000").unwrap();
    let values = vec![Value::String("Aé".into()), Value::String(String::new())];
    assert_eq!(decode(&bytes).unwrap(), values);
    assert_eq!(encode(&values).unwrap(), bytes);
}

#[test]
fn preserves_scalars_and_utf8_without_normalization_or_truncation() {
    let values = vec![
        Value::Word(ValueType::U256, [255; 32]),
        Value::String("é e\u{301}\0🪨".into()),
    ];
    assert_eq!(decode(&encode(&values).unwrap()).unwrap(), values);
    let limit = vec![Value::String("a".repeat(MAX_STRING_BYTES))];
    assert_eq!(decode(&encode(&limit).unwrap()).unwrap(), limit);
    assert!(encode(&[Value::String("a".repeat(MAX_STRING_BYTES + 1))]).is_err());
    assert!(encode(&vec![Value::String(String::new()); 65]).is_err());
    assert!(encode(&vec![limit[0].clone(); 16]).is_err());
}

#[test]
fn rejects_malformed_noncanonical_and_oversized_envelopes() {
    let bytes = encode(&[Value::String("Token".into())]).unwrap();
    for end in 0..bytes.len() {
        assert!(decode(&bytes[..end]).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode(&trailing).is_err());
    let mut version = bytes;
    version[4] = 2;
    assert!(decode(&version).is_err());
    for hex in [
        "4c56414c010001060001ff",
        "4c56414c010001070000",
        "4c56414c010041",
        "4c56414c010001061001",
    ] {
        assert!(decode(&hex::decode(hex).unwrap()).is_err());
    }
    let mut word = [0; 32];
    word[31] = 2;
    assert!(encode(&[Value::Word(ValueType::Bool, word)]).is_err());
    let mut boolean = hex::decode("4c56414c01000103").unwrap();
    boolean.extend(word);
    assert!(decode(&boolean).is_err());
    assert!(decode(&vec![0; MAX_ENVELOPE_BYTES + 1]).is_err());
}
