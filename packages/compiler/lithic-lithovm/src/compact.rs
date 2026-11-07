//! Encoding-only compaction, restricted to the new account-primitives candidate.
//! Legacy source artifacts through v15 must remain byte-identical.
use lithovm_bytecode::{Instruction, Program, ReturnValue, Statement, ACCOUNT_PRIMITIVES_VERSION};

pub(super) fn optimize(program: &mut Program) {
    if program.bytecode_version() < ACCOUNT_PRIMITIVES_VERSION {
        return;
    }
    visit_program(program, &mut |instruction| {
        if let Instruction::Constant(ty, word) = instruction {
            // A 0..30-byte minimal payload is strictly smaller than 32-byte PUSH.
            if word[..2] == [0; 2] {
                *instruction = Instruction::CompactConstant(*ty, *word);
            }
        }
    });
}

fn visit_program(program: &mut Program, visit: &mut impl FnMut(&mut Instruction)) {
    for function in &mut program.functions {
        match &mut function.return_value {
            ReturnValue::Expression(code) => code.iter_mut().for_each(&mut *visit),
            ReturnValue::Statements(body) => visit_body(body, visit),
            _ => {}
        }
    }
}

fn visit_body(body: &mut [Statement], visit: &mut impl FnMut(&mut Instruction)) {
    let expression = |code: &mut Vec<Instruction>, visit: &mut _| {
        code.iter_mut().for_each(visit);
    };
    for statement in body {
        match statement {
            Statement::PayloadInvoke {
                target,
                value,
                payload,
                ..
            } => {
                for code in [target, value, payload] {
                    expression(code, visit);
                }
            }
            Statement::Create {
                template,
                salt,
                initializer,
                value,
                arguments,
            } => {
                for code in [template, salt, initializer, value] {
                    expression(code, visit);
                }
                for (_, code) in arguments {
                    expression(code, visit);
                }
            }
            Statement::Invoke {
                target,
                selector,
                value,
                arguments,
                ..
            } => {
                for code in [target, selector, value] {
                    expression(code, visit);
                }
                for (_, code) in arguments {
                    expression(code, visit);
                }
            }
            Statement::Let {
                expression: code, ..
            }
            | Statement::LetMutable {
                expression: code, ..
            }
            | Statement::SetLocal {
                expression: code, ..
            }
            | Statement::Store {
                expression: code, ..
            }
            | Statement::Require(code)
            | Statement::Return(code) => expression(code, visit),
            Statement::MapStore {
                keys,
                expression: code,
                ..
            } => {
                for key in keys {
                    expression(key, visit);
                }
                expression(code, visit);
            }
            Statement::Emit { values, .. } => {
                for code in values {
                    expression(code, visit);
                }
            }
            Statement::Transfer { recipient, amount } => {
                expression(recipient, visit);
                expression(amount, visit);
            }
            Statement::Call {
                target,
                selector,
                value,
            } => {
                for code in [target, selector, value] {
                    expression(code, visit);
                }
            }
            Statement::Repeat { count, body } => {
                expression(count, visit);
                visit_body(body, visit);
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
            } => {
                expression(condition, visit);
                visit_body(then_branch, visit);
                visit_body(else_branch, visit);
            }
            Statement::Revert => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lithovm_bytecode::{parse, COMPACT_CONSTANT_VERSION};
    struct NoEffects;
    impl lithovm::ExecutionHost for NoEffects {
        fn emit(
            &mut self,
            _: &lithovm::EventRecord<lithovm_bytecode::values::Value>,
        ) -> Result<(), lithovm::ExecutionFailure> {
            panic!("unexpected event")
        }
        fn transfer(
            &mut self,
            _: &lithovm::NativeTransfer,
        ) -> Result<[u8; 32], lithovm::ExecutionFailure> {
            panic!("unexpected transfer")
        }
        fn invoke(
            &mut self,
            _: lithovm::NativeInvocation,
            _: &lithovm::Storage,
        ) -> Result<lithovm::InvocationResult, lithovm::ExecutionFailure> {
            panic!("unexpected call")
        }
    }

    fn expanded(mut program: Program) -> Program {
        visit_program(&mut program, &mut |instruction| {
            if let Instruction::CompactConstant(ty, word) = instruction {
                *instruction = Instruction::Constant(*ty, *word);
            }
        });
        program
    }

    #[test]
    fn account_compaction_changes_only_constant_encoding() {
        let source = include_str!(
            "../../../../sdk/contracts/standards/lep10015_native_account_candidate.lithic"
        );
        let artifact = crate::compile(source).unwrap();
        assert_eq!(artifact.bytecode_version, COMPACT_CONSTANT_VERSION);
        let code = hex::decode(&artifact.bytecode[2..]).unwrap();
        let original = expanded(parse(&code).unwrap());
        assert_eq!(original.bytecode_version(), ACCOUNT_PRIMITIVES_VERSION);
        let old_code = original.encode().unwrap();
        // Exact reviewed local v16 account: every other byte remains unchanged.
        assert_eq!(old_code.len(), 23_924);
        assert_eq!(
            crate::keccak_hex(&old_code),
            "0x18a4b57e792b0e357f3bf26ace42d899b68523ff064617585b2fa5204aba2cac"
        );
        assert!(code.len() < 9_000);
    }

    #[test]
    fn compact_and_expanded_execution_have_identical_results_gas_and_failures() {
        let artifact = crate::compile("contract C { pub fn x(data: bytes) -> u64 { let n: u64 = bytes_get(data, 0); require(n < 4); return n + 1; } }").unwrap();
        let compact = hex::decode(&artifact.bytecode[2..]).unwrap();
        let expanded = expanded(parse(&compact).unwrap()).encode().unwrap();
        assert_eq!(compact[7], COMPACT_CONSTANT_VERSION);
        assert_eq!(expanded[7], ACCOUNT_PRIMITIVES_VERSION);
        let vm = lithovm::Vm::default();
        let context = lithovm::ExecutionContext::default();
        let mut successes = 0;
        for data in [vec![], vec![0], vec![3], vec![4], vec![255]] {
            for gas in [1, 10, 30, 1_000] {
                let args = [lithovm_bytecode::values::Value::Bytes(data.clone())];
                let mut a = lithovm::Storage::default();
                let mut b = a.clone();
                let call = |code| lithovm::ValueCall {
                    bytecode: code,
                    function: "x",
                    arguments: &args,
                    gas_limit: gas,
                    context: &context,
                };
                let first = vm.execute_hosted(call(&compact), &mut a, &mut NoEffects);
                let second = vm.execute_hosted(call(&expanded), &mut b, &mut NoEffects);
                if matches!(first, lithovm::ExecutionOutcome::Success(_)) {
                    successes += 1;
                }
                assert_eq!(first, second);
                assert_eq!(a, b);
            }
        }
        assert!(
            successes > 0,
            "differential loop must exercise successful execution"
        );
    }

    #[test]
    fn old_programs_and_full_width_constants_are_not_compacted() {
        for source in [
            "contract Old { pub fn x(n: u64) -> u64 { return n + 1; } }",
            "contract Hash { pub fn x(data: bytes) -> bytes32 { return keccak256(data); } }",
        ] {
            let artifact = crate::compile(source).unwrap();
            let original = parse(&hex::decode(&artifact.bytecode[2..]).unwrap()).unwrap();
            let mut copy = original.clone();
            optimize(&mut copy);
            assert_eq!(copy, original);
        }
        let mut program = parse(
            &hex::decode(
                &crate::compile(
                    "contract C { pub fn x() -> address { return contract.address; } }",
                )
                .unwrap()
                .bytecode[2..],
            )
            .unwrap(),
        )
        .unwrap();
        program.functions.push(lithovm_bytecode::Function {
            name: "wide".into(),
            parameters: vec![],
            return_type: lithovm_bytecode::ValueType::Bytes32,
            return_value: ReturnValue::Expression(vec![Instruction::Constant(
                lithovm_bytecode::ValueType::Bytes32,
                [255; 32],
            )]),
        });
        let original = program.clone();
        optimize(&mut program);
        assert_eq!(program, original);
        assert_eq!(program.encode().unwrap()[7], ACCOUNT_PRIMITIVES_VERSION);
    }
}
