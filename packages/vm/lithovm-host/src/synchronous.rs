use super::*;
use lithovm::{
    charge_external_fuel, ExecutionHost, InvocationResult, NativeCreation, NativeInvocation,
    NativeTransfer, ValueCall,
};

pub(super) fn execute<T: StateTransaction>(
    vm: &Vm,
    state: &mut T,
    frame: FrameRequest<Value>,
    mut contract: DeployedContract,
    events: &mut EventJournal<Value>,
    active: &mut Vec<Address>,
) -> Result<(ExecutionResult<Value>, u64), HostFailure> {
    let context = ExecutionContext {
        caller: frame.caller,
        value: frame.value,
        block_height: frame.block_height,
        block_timestamp: frame.block_timestamp,
        chain_id: frame.chain_id,
        call_depth: frame.depth,
        contract_balance: load_balance(state, frame.contract, 0, frame.contract)?,
    };
    let mut adapter = Adapter {
        vm,
        state,
        address: frame.contract,
        context: &context,
        template: contract.clone(),
        events,
        active,
        failure: None,
    };
    let outcome = vm.execute_hosted(
        ValueCall {
            bytecode: &contract.bytecode,
            function: &frame.function,
            arguments: &frame.arguments,
            gas_limit: frame.gas_limit,
            context: &context,
        },
        &mut contract.storage,
        &mut adapter,
    );
    match outcome {
        ExecutionOutcome::Success(result) => {
            let gas = result.gas_used;
            store_contract(adapter.state, frame.contract, contract, gas)?;
            Ok((result, gas))
        }
        ExecutionOutcome::Failure(failure) => {
            // Preserve the actual failing child's identity, with the total gas
            // metered by the suspended/resumed caller rather than child gas alone.
            if let Some(mut original) = adapter.failure {
                original.gas_used = failure.gas_used;
                Err(original)
            } else {
                Err(vm_failure(frame.contract, failure))
            }
        }
    }
}

struct Adapter<'a, T> {
    vm: &'a Vm,
    state: &'a mut T,
    address: Address,
    context: &'a ExecutionContext,
    template: DeployedContract,
    events: &'a mut EventJournal<Value>,
    active: &'a mut Vec<Address>,
    failure: Option<HostFailure>,
}

impl<T: StateTransaction> Adapter<'_, T> {
    fn create_inner(
        &mut self,
        request: NativeCreation,
        storage: &Storage,
    ) -> Result<InvocationResult, HostFailure> {
        let template = load_contract(self.state, request.template, 0)?;
        if template.bytecode.len() > MAX_CHILD_CODE_BYTES {
            return Err(HostFailure {
                kind: HostFailureKind::InvalidBytecode,
                message: "child code exceeds creation limit".into(),
                gas_used: 0,
                failed_contract: request.template,
            });
        }
        let creation_gas = CHILD_CREATE_BASE_GAS + template.bytecode.len() as u64;
        if creation_gas > request.gas_limit {
            charge_external_fuel(request.gas_limit);
            return Err(HostFailure {
                kind: HostFailureKind::Vm(FailureKind::OutOfGas),
                message: "out of gas creating child code".into(),
                gas_used: request.gas_limit,
                failed_contract: self.address,
            });
        }
        if !charge_external_fuel(creation_gas) {
            return Err(HostFailure {
                kind: HostFailureKind::Vm(FailureKind::OutOfGas),
                message: "host fuel exhausted creating child code".into(),
                gas_used: request.gas_limit,
                failed_contract: self.address,
            });
        }
        let effective_salt = caller_bound_salt(self.context.caller, request.salt);
        let address = child_contract_address(
            self.address,
            effective_salt,
            template.code_hash,
            self.context.chain_id,
        );
        // Everything after this point, including validation failures, pays for code.
        let created = (|| {
            if self.events.deployments.len() >= 64 {
                return Err(HostFailure {
                    kind: HostFailureKind::Vm(FailureKind::Trap),
                    message: "transaction deployment record limit exceeded".into(),
                    gas_used: 0,
                    failed_contract: self.address,
                });
            }
            if self
                .state
                .load_contract(&address)
                .map_err(|e| state_failure(e, 0, address))?
                .is_some()
            {
                return Err(HostFailure {
                    kind: HostFailureKind::ContractExists,
                    message: "derived child address is already deployed".into(),
                    gas_used: 0,
                    failed_contract: address,
                });
            }
            let program = parse(&template.bytecode).map_err(|e| HostFailure {
                kind: HostFailureKind::InvalidBytecode,
                message: e.to_string(),
                gas_used: 0,
                failed_contract: request.template,
            })?;
            let entrypoints = validated_entrypoints(&program, address)?;
            if let Some(initializer) = program.functions.iter().find(|f| f.name == "initialize") {
                if request.initializer != function_selector(initializer) {
                    return Err(HostFailure {
                        kind: if entrypoints.contains_key(&request.initializer) {
                            HostFailureKind::InvalidEntrypoint
                        } else {
                            HostFailureKind::UnknownSelector
                        },
                        message: "child declaring initialize requires its initialize selector"
                            .into(),
                        gas_used: 0,
                        failed_contract: address,
                    });
                }
            }
            // The template is code only. Never copy its storage or native balance.
            store_contract(
                self.state,
                address,
                DeployedContract {
                    bytecode: template.bytecode,
                    code_hash: template.code_hash,
                    storage: Storage::default(),
                    entrypoints,
                },
                0,
            )?;
            self.events.deployments.push(CommittedDeployment {
                creator: self.address,
                contract: address,
                code_hash: template.code_hash,
                origin: DeploymentOrigin::Child {
                    template: request.template,
                    salt: effective_salt,
                },
            });
            let result = self.invoke_inner(
                NativeInvocation {
                    target: address,
                    selector: request.initializer,
                    value: request.value,
                    arguments: request.arguments,
                    return_type: lithovm_bytecode::ValueType::Bool,
                    gas_limit: request.gas_limit - creation_gas,
                },
                storage,
            )?;
            let mut yes = [0; 32];
            yes[31] = 1;
            if result.value != Value::Word(lithovm_bytecode::ValueType::Bool, yes) {
                return Err(HostFailure {
                    kind: HostFailureKind::Vm(FailureKind::Revert),
                    message: "child initializer did not return true".into(),
                    gas_used: result.gas_used,
                    failed_contract: address,
                });
            }
            Ok(InvocationResult {
                value: Value::Word(lithovm_bytecode::ValueType::Address, address),
                gas_used: result.gas_used,
                caller_balance: result.caller_balance,
            })
        })();
        match created {
            Ok(mut result) => {
                result.gas_used += creation_gas;
                Ok(result)
            }
            Err(mut failure) => {
                failure.gas_used += creation_gas;
                Err(failure)
            }
        }
    }

    fn remember(&mut self, failure: HostFailure) -> ExecutionFailure {
        let vm_failure = ExecutionFailure {
            kind: match failure.kind {
                HostFailureKind::Vm(kind) => kind,
                _ => FailureKind::Trap,
            },
            message: failure.message.clone(),
            gas_used: failure.gas_used,
        };
        self.failure = Some(failure);
        vm_failure
    }

    fn transfer_value(
        &mut self,
        recipient: Address,
        amount: [u8; 32],
    ) -> Result<[u8; 32], HostFailure> {
        let balance = load_balance(self.state, self.address, 0, self.address)?;
        let remaining = debit(balance, amount).ok_or_else(|| HostFailure {
            kind: HostFailureKind::InsufficientBalance,
            message: "native value exceeds contract balance".into(),
            gas_used: 0,
            failed_contract: self.address,
        })?;
        store_balance(self.state, self.address, remaining, 0, self.address)?;
        let balance = load_balance(self.state, recipient, 0, self.address)?;
        let credited = credit(balance, amount).ok_or_else(|| {
            state_failure("native recipient balance overflow".into(), 0, self.address)
        })?;
        store_balance(self.state, recipient, credited, 0, self.address)?;
        load_balance(self.state, self.address, 0, self.address)
    }

    fn invoke_inner(
        &mut self,
        request: NativeInvocation,
        storage: &Storage,
    ) -> Result<InvocationResult, HostFailure> {
        if self.active.contains(&request.target) {
            return Err(HostFailure {
                kind: HostFailureKind::Reentrancy,
                message: "reentrant contract call is not permitted".into(),
                gas_used: 0,
                failed_contract: request.target,
            });
        }
        let child = load_contract(self.state, request.target, 0)?;
        let function = child
            .entrypoints
            .get(&request.selector)
            .cloned()
            .ok_or_else(|| HostFailure {
                kind: HostFailureKind::UnknownSelector,
                message: "contract selector is not registered".into(),
                gas_used: 0,
                failed_contract: request.target,
            })?;
        let program = parse(&child.bytecode).map_err(|error| HostFailure {
            kind: HostFailureKind::InvalidBytecode,
            message: error.to_string(),
            gas_used: 0,
            failed_contract: request.target,
        })?;
        let definition = program.functions.iter().find(|f| f.name == function);
        if !definition.is_some_and(|f| f.return_type == request.return_type) {
            return Err(HostFailure {
                kind: HostFailureKind::InvalidEntrypoint,
                message: "invoke return type does not match callee".into(),
                gas_used: 0,
                failed_contract: request.target,
            });
        }
        let mut caller = self.template.clone();
        caller.storage = storage.clone();
        store_contract(self.state, self.address, caller, 0)?;
        self.transfer_value(request.target, request.value)?;
        let frame = FrameRequest {
            contract: request.target,
            function,
            arguments: request.arguments,
            gas_limit: request.gas_limit,
            caller: self.address,
            value: request.value,
            block_height: self.context.block_height,
            block_timestamp: self.context.block_timestamp,
            chain_id: self.context.chain_id,
            depth: self.context.call_depth + 1,
        };
        self.active.push(request.target);
        let result = execute_frame(self.vm, self.state, frame, self.events, self.active);
        self.active.pop();
        let (result, gas_used) = result?;
        Ok(InvocationResult {
            value: result.return_value,
            gas_used,
            caller_balance: load_balance(self.state, self.address, gas_used, self.address)?,
        })
    }
}

impl<T: StateTransaction> ExecutionHost for Adapter<'_, T> {
    fn create(
        &mut self,
        request: NativeCreation,
        storage: &Storage,
    ) -> Result<InvocationResult, ExecutionFailure> {
        self.create_inner(request, storage)
            .map_err(|failure| self.remember(failure))
    }
    fn emit(&mut self, event: &EventRecord<Value>) -> Result<(), ExecutionFailure> {
        let size = 7 + event
            .fields
            .iter()
            .map(|(_, _, value)| Value::encoded_size(value))
            .sum::<usize>();
        if self.events.value_bytes + size > lithovm_bytecode::values::MAX_ENVELOPE_BYTES {
            let failure = HostFailure {
                kind: HostFailureKind::Vm(FailureKind::Trap),
                message: "host event output exceeds byte limit".into(),
                gas_used: 0,
                failed_contract: self.address,
            };
            return Err(self.remember(failure));
        }
        self.events.value_bytes += size;
        self.events.events.push(CommittedEvent {
            contract: self.address,
            event: event.clone(),
        });
        Ok(())
    }

    fn transfer(&mut self, transfer: &NativeTransfer) -> Result<[u8; 32], ExecutionFailure> {
        self.transfer_value(transfer.recipient, transfer.amount)
            .map_err(|failure| self.remember(failure))
    }

    fn invoke(
        &mut self,
        request: NativeInvocation,
        storage: &Storage,
    ) -> Result<InvocationResult, ExecutionFailure> {
        self.invoke_inner(request, storage)
            .map_err(|failure| self.remember(failure))
    }
}
