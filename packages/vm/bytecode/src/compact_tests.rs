use super::*;

fn program(instruction: Instruction, ty: ValueType) -> Program {
    Program {
        storage: vec![],
        maps: vec![],
        events: vec![],
        functions: vec![Function {
            name: "value".into(),
            parameters: vec![],
            return_type: ty,
            return_value: ReturnValue::Expression(vec![instruction]),
        }],
    }
}

#[test]
fn compact_constants_round_trip_minimal_words_and_preserve_legacy_encoding() {
    for ty in [
        ValueType::U64,
        ValueType::U256,
        ValueType::Bool,
        ValueType::Address,
        ValueType::Bytes32,
    ] {
        for len in 0..=30 {
            let mut word = [0; 32];
            if len != 0 {
                word[32 - len..].fill(1);
            }
            let compact = program(Instruction::CompactConstant(ty, word), ty);
            if validate_word(ty, &word).is_err() {
                assert!(compact.encode().is_err());
                continue;
            }
            let encoded = compact.encode().unwrap();
            assert_eq!(encoded[7], COMPACT_CONSTANT_VERSION);
            assert_eq!(parse(&encoded).unwrap(), compact);
            let legacy = program(Instruction::Constant(ty, word), ty)
                .encode()
                .unwrap();
            assert_eq!(legacy[7], VERSION);
            assert_eq!(legacy.len() - encoded.len(), 31 - len);
            for old_version in LEGACY_VERSION..=ACCOUNT_PRIMITIVES_VERSION {
                let mut reader = Reader {
                    bytes: &encoded[encoded.len() - 3 - len..],
                    position: 0,
                };
                assert!(decode_instruction(&mut reader, old_version).is_err());
            }
        }
    }
    assert!(program(
        Instruction::CompactConstant(ValueType::U256, [1; 32]),
        ValueType::U256
    )
    .encode()
    .is_err());
}

#[test]
fn compact_constants_reject_malleable_truncated_oversized_and_ill_typed_forms() {
    let malformed = [
        vec![32, 2],
        vec![32, 2, 1],
        vec![32, 2, 1, 0],
        vec![32, 2, 2, 0, 1],
        vec![32, 2, 31],
        vec![32, 2, 32],
        vec![32, 2, 255],
        vec![32, 255, 0],
        vec![32, 6, 0],
        vec![32, 7, 0],
        vec![32, 3, 1, 2],                       // bool must be 0/1
        [vec![32, 1, 9], vec![1; 9]].concat(),   // u64 high bits
        [vec![32, 4, 21], vec![1; 21]].concat(), // address high bits
    ];
    for bytes in malformed {
        let mut reader = Reader {
            bytes: &bytes,
            position: 0,
        };
        assert!(
            decode_instruction(&mut reader, COMPACT_CONSTANT_VERSION).is_err(),
            "{bytes:?}"
        );
    }
    for ty in [ValueType::String, ValueType::Bytes] {
        assert!(program(Instruction::CompactConstant(ty, [0; 32]), ty)
            .encode()
            .is_err());
    }
}

#[test]
fn compact_constant_in_payload_operand_selects_v17_and_rejects_downgrade() {
    let program = Program {
        storage: vec![],
        maps: vec![],
        events: vec![],
        functions: vec![Function {
            name: "read".into(),
            parameters: vec![ValueType::Address, ValueType::Bytes],
            return_type: ValueType::Bytes,
            return_value: ReturnValue::Statements(vec![
                Statement::PayloadInvoke {
                    read_only: true,
                    target: vec![Instruction::Parameter(0)],
                    value: vec![Instruction::CompactConstant(ValueType::U256, [0; 32])],
                    payload: vec![Instruction::Parameter(1)],
                },
                Statement::Return(vec![Instruction::Local(0)]),
            ]),
        }],
    };
    let mut code = program.encode().unwrap();
    assert_eq!(code[7], COMPACT_CONSTANT_VERSION);
    assert_eq!(parse(&code).unwrap(), program);
    code[7] = ACCOUNT_PRIMITIVES_VERSION;
    assert!(parse(&code).is_err());
}
