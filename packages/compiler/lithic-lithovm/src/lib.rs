//! Fail-closed Lithic-to-native-LithoVM backend.
//!
//! The backend emits a versioned, strictly decoded native artifact. Unsupported
//! declarations reject the whole compilation; no source behavior is silently
//! discarded.

use lithic_syntax::{Contract, Item, Type};
use lithovm_bytecode::{
    function_selector, function_signature, EventDefinition, Function, Instruction, MapField,
    Program, ReturnValue, Statement, StorageField, ValueType, MAX_BLOCK_DEPTH, MAX_LOCALS,
    MAX_STATEMENTS,
};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::fmt;

pub const TARGET: &str = "lithovm-native-v11";
pub const ARTIFACT_VERSION: u8 = 1;
pub mod verification;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntrypointArtifact {
    pub name: String,
    pub signature: String,
    pub selector: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompiledConstant {
    name: String,
    value_type: ValueType,
    word: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub artifact_version: u8,
    pub compiler: String,
    pub compiler_version: String,
    pub contract_name: String,
    pub target: String,
    pub bytecode_version: u8,
    pub source_hash: String,
    pub code_hash: String,
    pub entrypoints: Vec<EntrypointArtifact>,
    pub abi: serde_json::Value,
    pub bytecode: String,
}

impl Artifact {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("artifact serialization cannot fail")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileError {
    messages: Vec<String>,
}

impl CompileError {
    fn one(message: impl Into<String>) -> Self {
        Self {
            messages: vec![message.into()],
        }
    }

    pub fn messages(&self) -> &[String] {
        &self.messages
    }
}

impl fmt::Display for CompileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.messages.join("\n"))
    }
}

impl std::error::Error for CompileError {}

pub fn compile(source: &str) -> Result<Artifact, CompileError> {
    let parsed = lithic_syntax::parse(source);
    let mut errors: Vec<String> = parsed
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.is_error())
        .map(|diagnostic| format!("syntax error: {}", diagnostic.message))
        .collect();
    let contract = parsed
        .contract
        .ok_or_else(|| CompileError::one("no contract found"))?;
    errors.extend(
        lithic_syntax::check(&contract)
            .into_iter()
            .filter(|finding| matches!(finding.level, lithic_syntax::Level::Error))
            .map(|finding| format!("declaration error: {}", finding.message)),
    );
    if !errors.is_empty() {
        return Err(CompileError { messages: errors });
    }
    compile_contract(&contract, source)
}

fn compile_contract(contract: &Contract, source: &str) -> Result<Artifact, CompileError> {
    let mut errors = Vec::new();
    let mut functions = Vec::new();
    let mut abi = Vec::new();
    let mut storage = Vec::new();
    let mut maps = Vec::new();
    let mut events = Vec::new();
    let mut constants = Vec::new();

    for item in &contract.items {
        if let Item::Const(constant) = item {
            match lower_type(&constant.ty).and_then(|value_type| {
                compile_constant_value(&constant.value_src, value_type)
                    .map(|word| (value_type, word))
            }) {
                Ok((value_type, word)) => constants.push(CompiledConstant {
                    name: constant.name.clone(),
                    value_type,
                    word,
                }),
                Err(message) => errors.push(format!("constant '{}': {message}", constant.name)),
            }
        }
    }

    for item in &contract.items {
        if let Item::State(state) = item {
            for field in &state.fields {
                match lower_type(&field.ty) {
                    Ok(value_type) => storage.push(StorageField {
                        name: field.name.clone(),
                        value_type,
                    }),
                    Err(_) => match lower_map_type(&field.ty) {
                        Ok((key_types, value_type)) => maps.push(MapField {
                            name: field.name.clone(),
                            key_types,
                            value_type,
                        }),
                        Err(message) => {
                            errors.push(format!("state field '{}': {message}", field.name))
                        }
                    },
                }
            }
        }
    }

    for item in &contract.items {
        if let Item::Event(event) = item {
            let mut fields = Vec::new();
            for field in &event.fields {
                match lower_type(&field.ty) {
                    Ok(value_type) => fields.push(StorageField {
                        name: field.name.clone(),
                        value_type,
                    }),
                    Err(message) => errors.push(format!(
                        "event '{}' field '{}': {message}",
                        event.name, field.name
                    )),
                }
            }
            events.push(EventDefinition {
                name: event.name.clone(),
                fields,
            });
            abi.push(serde_json::json!({
                "type": "event",
                "name": event.name,
                "inputs": event.fields.iter().filter_map(|field| lower_type(&field.ty).ok().map(|value_type| {
                    serde_json::json!({"name": field.name, "type": value_type.name()})
                })).collect::<Vec<_>>()
            }));
        }
    }

    for item in &contract.items {
        match item {
            Item::Const(_) => {}
            Item::Event(_) => {}
            Item::Func(function) => {
                match compile_function(function, &constants, &storage, &maps, &events) {
                    Ok((compiled, entry)) => {
                        functions.push(compiled);
                        abi.push(entry);
                    }
                    Err(message) => errors.push(format!("function '{}': {message}", function.name)),
                }
            }
            Item::State(_) => {}
        }
    }
    if functions.is_empty() {
        errors.push("at least one supported public function is required".to_string());
    }
    if !errors.is_empty() {
        return Err(CompileError { messages: errors });
    }

    let program = Program {
        storage,
        maps,
        events,
        functions,
    };
    let bytes = program
        .encode()
        .map_err(|error| CompileError::one(format!("bytecode encoding failed: {error}")))?;
    let entrypoints = program
        .functions
        .iter()
        .map(|function| EntrypointArtifact {
            name: function.name.clone(),
            signature: function_signature(function),
            selector: format!("0x{}", hex::encode(function_selector(function))),
        })
        .collect();
    Ok(Artifact {
        artifact_version: ARTIFACT_VERSION,
        compiler: "lithc".to_string(),
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        contract_name: contract.name.clone(),
        target: format!("lithovm-native-v{}", program.bytecode_version()),
        bytecode_version: program.bytecode_version(),
        source_hash: keccak_hex(source.as_bytes()),
        code_hash: keccak_hex(&bytes),
        entrypoints,
        abi: serde_json::Value::Array(abi),
        bytecode: format!("0x{}", hex::encode(bytes)),
    })
}

fn keccak_hex(bytes: &[u8]) -> String {
    format!("0x{}", hex::encode(Keccak256::digest(bytes)))
}

fn compile_function(
    function: &lithic_syntax::FuncDecl,
    constants: &[CompiledConstant],
    storage: &[StorageField],
    maps: &[MapField],
    events: &[EventDefinition],
) -> Result<(Function, serde_json::Value), String> {
    if !function.is_pub {
        return Err("private functions are unsupported".to_string());
    }
    if function.is_async {
        return Err("async functions are unsupported".to_string());
    }
    if !function.attrs.is_empty() {
        return Err("function attributes are unsupported".to_string());
    }
    let return_type = lower_type(
        function
            .ret
            .as_ref()
            .ok_or_else(|| "a return type is required".to_string())?,
    )?;
    let parameters = function
        .params
        .iter()
        .map(|parameter| lower_type(&parameter.ty))
        .collect::<Result<Vec<_>, _>>()?;
    let return_value = parse_body(
        function,
        return_type,
        &parameters,
        constants,
        storage,
        maps,
        events,
    )?;
    let abi = serde_json::json!({
        "type": "function",
        "name": function.name,
        "inputs": function.params.iter().zip(&parameters).map(|(parameter, value_type)| {
            serde_json::json!({"name": parameter.name, "type": value_type.name()})
        }).collect::<Vec<_>>(),
        "outputs": [{"type": return_type.name()}]
    });
    Ok((
        Function {
            name: function.name.clone(),
            parameters,
            return_type,
            return_value,
        },
        abi,
    ))
}

fn lower_type(value: &Type) -> Result<ValueType, String> {
    match value {
        Type::Named(name) => match name.as_str() {
            "u64" => Ok(ValueType::U64),
            "u256" => Ok(ValueType::U256),
            "bool" => Ok(ValueType::Bool),
            "address" => Ok(ValueType::Address),
            "bytes32" => Ok(ValueType::Bytes32),
            "string" => Ok(ValueType::String),
            other => Err(format!("type '{other}' has no native LithoVM v11 lowering")),
        },
        Type::Map(_, _) | Type::Vec(_) => Err("collection types are unsupported".to_string()),
    }
}

fn lower_map_type(value: &Type) -> Result<(Vec<ValueType>, ValueType), String> {
    let Type::Map(key, value) = value else {
        return Err("collection type must be a map".to_string());
    };
    let mut key_types =
        vec![lower_type(key).map_err(|_| "map keys must be scalar LithoVM values".to_string())?];
    let value_type = match value.as_ref() {
        Type::Map(_, _) => {
            let (nested_keys, value_type) = lower_map_type(value)?;
            key_types.extend(nested_keys);
            value_type
        }
        scalar => lower_type(scalar)
            .map_err(|_| "map values must be scalar values or nested maps".to_string())?,
    };
    Ok((key_types, value_type))
}

fn lower_named_type(name: &str) -> Result<ValueType, String> {
    match name {
        "u64" => Ok(ValueType::U64),
        "u256" => Ok(ValueType::U256),
        "bool" => Ok(ValueType::Bool),
        "address" => Ok(ValueType::Address),
        "bytes32" => Ok(ValueType::Bytes32),
        "string" => Ok(ValueType::String),
        other => Err(format!("type '{other}' has no native LithoVM v11 lowering")),
    }
}

fn parse_body(
    function: &lithic_syntax::FuncDecl,
    return_type: ValueType,
    parameter_types: &[ValueType],
    constants: &[CompiledConstant],
    storage: &[StorageField],
    maps: &[MapField],
    events: &[EventDefinition],
) -> Result<ReturnValue, String> {
    let body = function.body_src.trim();
    if starts_with_keyword(body, "return") {
        return parse_return(
            function,
            return_type,
            parameter_types,
            constants,
            storage,
            maps,
        );
    }
    let parameter_names = function
        .params
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>();
    let mut parser = BodyParser {
        source: body,
        position: 0,
        parameter_names,
        parameter_types,
        local_names: Vec::new(),
        local_types: Vec::new(),
        local_mutability: Vec::new(),
        constants,
        storage,
        maps,
        events,
        statement_count: 0,
        return_type,
    };
    let statements = parser.parse_block(false, 0, true)?;
    Ok(ReturnValue::Statements(statements))
}

fn starts_with_keyword(source: &str, keyword: &str) -> bool {
    source
        .strip_prefix(keyword)
        .and_then(|remaining| remaining.as_bytes().first())
        .is_some_and(u8::is_ascii_whitespace)
}

struct BodyParser<'a> {
    source: &'a str,
    position: usize,
    parameter_names: Vec<&'a str>,
    parameter_types: &'a [ValueType],
    local_names: Vec<String>,
    local_types: Vec<ValueType>,
    local_mutability: Vec<bool>,
    constants: &'a [CompiledConstant],
    storage: &'a [StorageField],
    maps: &'a [MapField],
    events: &'a [EventDefinition],
    statement_count: usize,
    return_type: ValueType,
}

fn is_terminal(statement: &Statement) -> bool {
    match statement {
        Statement::Return(_) | Statement::Revert => true,
        Statement::If {
            then_branch,
            else_branch,
            ..
        } => {
            then_branch.last().is_some_and(is_terminal)
                && else_branch.last().is_some_and(is_terminal)
        }
        _ => false,
    }
}

// v11 requires nonempty encoded blocks. A metered require(true) represents
// a source-level empty branch without changing the bytecode format.
fn nonempty_branch(statements: Vec<Statement>) -> Vec<Statement> {
    if statements.is_empty() {
        vec![Statement::Require(vec![Instruction::Constant(
            ValueType::Bool,
            word_from_u64(1),
        )])]
    } else {
        statements
    }
}

impl BodyParser<'_> {
    fn parse_block(
        &mut self,
        nested: bool,
        depth: usize,
        require_return: bool,
    ) -> Result<Vec<Statement>, String> {
        if depth > MAX_BLOCK_DEPTH {
            return Err(format!(
                "statement nesting exceeds maximum depth {MAX_BLOCK_DEPTH}"
            ));
        }
        let mut statements = Vec::new();
        let mut terminal = false;
        loop {
            self.skip_whitespace();
            if nested && self.peek_byte() == Some(b'}') {
                self.position += 1;
                if require_return && !terminal {
                    return Err("branch does not return on every path".to_string());
                }
                return Ok(statements);
            }
            if self.position == self.source.len() {
                if nested {
                    return Err("unterminated statement block".to_string());
                }
                if require_return && !terminal {
                    return Err("function does not return on every path".to_string());
                }
                return Ok(statements);
            }
            if terminal {
                return Err(
                    "unreachable statement after terminal return, revert, or branch".to_string(),
                );
            }

            let statement = if self.consume_keyword("let") {
                self.parse_let()?
            } else if self.consume_keyword("return") {
                terminal = true;
                self.parse_return_statement()?
            } else if self.consume_keyword("if") {
                let statement = self.parse_if(depth)?;
                terminal = is_terminal(&statement);
                statement
            } else if self.consume_keyword("repeat") {
                self.parse_repeat(depth)?
            } else if self.consume_keyword("require") {
                self.parse_require()?
            } else if self.consume_keyword("revert") {
                terminal = true;
                self.parse_revert()?
            } else if self.consume_keyword("emit") {
                self.parse_emit()?
            } else if self.consume_keyword("transfer_native") {
                self.parse_transfer()?
            } else if self.consume_keyword("call_contract") {
                self.parse_call()?
            } else if self.consume_self_prefix() {
                self.parse_store()?
            } else {
                self.parse_local_assignment()?
            };
            self.statement_count += 1;
            if self.statement_count > MAX_STATEMENTS {
                return Err(format!("function exceeds {MAX_STATEMENTS} statements"));
            }
            statements.push(statement);
        }
    }

    fn parse_let(&mut self) -> Result<Statement, String> {
        self.skip_whitespace();
        let mutable = self.consume_keyword("mut");
        let name = self.parse_identifier()?;
        if self.parameter_names.contains(&name.as_str()) || self.local_names.contains(&name) {
            return Err(format!("duplicate local binding '{name}'"));
        }
        self.skip_whitespace();
        let annotated_type = if self.consume_byte(b':') {
            self.skip_whitespace();
            Some(lower_named_type(&self.parse_identifier()?)?)
        } else {
            None
        };
        self.skip_whitespace();
        self.expect_byte(b'=', "expected '=' in local binding")?;
        self.skip_whitespace();
        let creation = self.consume_keyword("create_contract");
        if creation || self.consume_keyword("invoke") {
            if mutable {
                return Err("host result bindings must be immutable".into());
            }
            let return_type =
                annotated_type.ok_or("host result requires an explicit return type")?;
            if creation && return_type != ValueType::Address {
                return Err("create_contract result must be address".into());
            }
            self.skip_whitespace();
            self.expect_byte(b'(', "expected '(' after host operation")?;
            let source = self.take_expression_until(b')')?.to_owned();
            self.skip_whitespace();
            self.expect_byte(b';', "expected ';' after host operation")?;
            let parts = source.split(',').collect::<Vec<_>>();
            let operand_types = if creation {
                vec![
                    ValueType::Address,
                    ValueType::Bytes32,
                    ValueType::Bytes32,
                    ValueType::U256,
                ]
            } else {
                vec![ValueType::Address, ValueType::Bytes32, ValueType::U256]
            };
            if parts.len() < operand_types.len() || parts.len() > operand_types.len() + 64 {
                return Err(
                    "host operation requires its fixed operands and at most 64 arguments".into(),
                );
            }
            let mut expressions = parts
                .iter()
                .map(|part| self.compile_expression(part))
                .collect::<Result<Vec<_>, _>>()?;
            for (index, ty) in operand_types.into_iter().enumerate() {
                coerce_expression(&mut expressions[index], ty)?;
            }
            let mut expressions = expressions.into_iter();
            let target = expressions.next().unwrap().0;
            let selector = expressions.next().unwrap().0;
            let initializer = if creation {
                Some(expressions.next().unwrap().0)
            } else {
                None
            };
            let value = expressions.next().unwrap().0;
            let arguments = expressions
                .map(|(expression, ty)| (ty, expression))
                .collect();
            if self.local_names.len() >= MAX_LOCALS {
                return Err("too many local bindings".into());
            }
            self.local_names.push(name);
            self.local_types.push(return_type);
            self.local_mutability.push(false);
            if let Some(initializer) = initializer {
                return Ok(Statement::Create {
                    template: target,
                    salt: selector,
                    initializer,
                    value,
                    arguments,
                });
            }
            return Ok(Statement::Invoke {
                return_type,
                target,
                selector,
                value,
                arguments,
            });
        }
        let expression_source = self.take_expression_until(b';')?.to_owned();
        let (expression, inferred_type) = self.compile_expression(&expression_source)?;
        let value_type = annotated_type.unwrap_or(inferred_type);
        if value_type != inferred_type {
            return Err(format!(
                "local binding '{name}' has type {}, expression has type {}",
                value_type.name(),
                inferred_type.name()
            ));
        }
        if self.local_names.len() >= MAX_LOCALS {
            return Err(format!("function exceeds {MAX_LOCALS} local bindings"));
        }
        self.local_names.push(name);
        self.local_types.push(value_type);
        self.local_mutability.push(mutable);
        if mutable {
            Ok(Statement::LetMutable {
                value_type,
                expression,
            })
        } else {
            Ok(Statement::Let {
                value_type,
                expression,
            })
        }
    }

    fn parse_local_assignment(&mut self) -> Result<Statement, String> {
        let name = self.parse_identifier()?;
        let Some(local) = self
            .local_names
            .iter()
            .position(|candidate| candidate == &name)
        else {
            if self.parameter_names.contains(&name.as_str()) {
                return Err(format!("parameter '{name}' is immutable"));
            }
            return Err(format!("unknown local binding '{name}'"));
        };
        if !self.local_mutability[local] {
            return Err(format!("local binding '{name}' is immutable"));
        }
        self.skip_whitespace();
        self.expect_byte(b'=', "expected '=' in local assignment")?;
        let expression_source = self.take_expression_until(b';')?.to_owned();
        let (expression, expression_type) = self.compile_expression(&expression_source)?;
        let value_type = self.local_types[local];
        if expression_type != value_type {
            return Err(format!(
                "local binding '{name}' has type {}, expression has type {}",
                value_type.name(),
                expression_type.name()
            ));
        }
        Ok(Statement::SetLocal {
            local: local as u16,
            expression,
        })
    }

    fn parse_return_statement(&mut self) -> Result<Statement, String> {
        let expression_source = self.take_expression_until(b';')?.to_owned();
        let (expression, expression_type) = self.compile_expression(&expression_source)?;
        if expression_type != self.return_type {
            return Err(format!(
                "return expression has type {}, expected {}",
                expression_type.name(),
                self.return_type.name()
            ));
        }
        Ok(Statement::Return(expression))
    }

    fn parse_require(&mut self) -> Result<Statement, String> {
        self.skip_whitespace();
        self.expect_byte(b'(', "expected '(' after require")?;
        let condition_source = self.take_expression_until(b')')?.to_owned();
        let (condition, condition_type) = self.compile_expression(&condition_source)?;
        if condition_type != ValueType::Bool {
            return Err(format!(
                "require condition has type {}, expected bool",
                condition_type.name()
            ));
        }
        self.skip_whitespace();
        self.expect_byte(b';', "expected ';' after require")?;
        Ok(Statement::Require(condition))
    }

    fn parse_revert(&mut self) -> Result<Statement, String> {
        self.skip_whitespace();
        self.expect_byte(b'(', "expected '(' after revert")?;
        self.skip_whitespace();
        self.expect_byte(b')', "revert does not accept arguments")?;
        self.skip_whitespace();
        self.expect_byte(b';', "expected ';' after revert")?;
        Ok(Statement::Revert)
    }

    fn parse_store(&mut self) -> Result<Statement, String> {
        let field_name = self.parse_identifier()?;
        if let Some(field) = self
            .storage
            .iter()
            .position(|candidate| candidate.name == field_name)
        {
            self.skip_whitespace();
            self.expect_byte(b'=', "expected '=' in storage assignment")?;
            let expression_source = self.take_expression_until(b';')?.to_owned();
            let mut expression = self.compile_expression(&expression_source)?;
            let field_type = self.storage[field].value_type;
            coerce_expression(&mut expression, field_type).map_err(|_| {
                format!(
                    "storage field '{field_name}' has type {}, expression has type {}",
                    field_type.name(),
                    expression.1.name()
                )
            })?;
            return Ok(Statement::Store {
                field: field as u16,
                expression: expression.0,
            });
        }
        let map = self
            .maps
            .iter()
            .position(|candidate| candidate.name == field_name)
            .ok_or_else(|| format!("unknown storage field '{field_name}'"))?;
        let definition = &self.maps[map];
        let mut keys = Vec::with_capacity(definition.key_types.len());
        for key_type in &definition.key_types {
            self.skip_whitespace();
            self.expect_byte(b'[', "expected '[' before map key")?;
            let key_source = self.take_expression_until(b']')?.to_owned();
            let mut key = self.compile_expression(&key_source)?;
            coerce_expression(&mut key, *key_type).map_err(|_| {
                format!(
                    "map '{field_name}' key has type {}, expected {}",
                    key.1.name(),
                    key_type.name()
                )
            })?;
            keys.push(key.0);
        }
        self.skip_whitespace();
        if self.peek_byte() == Some(b'[') {
            return Err(format!("map '{field_name}' received too many keys"));
        }
        self.expect_byte(b'=', "expected '=' in map assignment")?;
        let expression_source = self.take_expression_until(b';')?.to_owned();
        let mut expression = self.compile_expression(&expression_source)?;
        coerce_expression(&mut expression, definition.value_type).map_err(|_| {
            format!(
                "map '{field_name}' has value type {}, expression has type {}",
                definition.value_type.name(),
                expression.1.name()
            )
        })?;
        Ok(Statement::MapStore {
            map: map as u16,
            keys,
            expression: expression.0,
        })
    }

    fn parse_emit(&mut self) -> Result<Statement, String> {
        let event_name = self.parse_identifier()?;
        let event = self
            .events
            .iter()
            .position(|candidate| candidate.name == event_name)
            .ok_or_else(|| format!("unknown event '{event_name}'"))?;
        self.skip_whitespace();
        self.expect_byte(b'{', "expected '{' after event name")?;
        let definition = &self.events[event];
        let mut values = Vec::with_capacity(definition.fields.len());
        if definition.fields.is_empty() {
            self.skip_whitespace();
            self.expect_byte(b'}', "expected '}' after empty event")?;
            self.skip_whitespace();
            self.consume_byte(b';');
            return Ok(Statement::Emit {
                event: event as u16,
                values,
            });
        }
        for (index, field) in definition.fields.iter().enumerate() {
            self.skip_whitespace();
            let supplied_name = self.parse_identifier()?;
            if supplied_name != field.name {
                return Err(format!(
                    "event '{event_name}' expected field '{}', found '{supplied_name}'",
                    field.name
                ));
            }
            self.skip_whitespace();
            self.expect_byte(b':', "expected ':' after event field name")?;
            let terminator = if index + 1 == definition.fields.len() {
                b'}'
            } else {
                b','
            };
            let expression_source = self.take_expression_until(terminator)?.to_owned();
            let (expression, expression_type) = self.compile_expression(&expression_source)?;
            if expression_type != field.value_type {
                return Err(format!(
                    "event '{}' field '{}' has type {}, expression has type {}",
                    event_name,
                    field.name,
                    field.value_type.name(),
                    expression_type.name()
                ));
            }
            values.push(expression);
        }
        self.skip_whitespace();
        self.consume_byte(b';');
        Ok(Statement::Emit {
            event: event as u16,
            values,
        })
    }

    fn parse_transfer(&mut self) -> Result<Statement, String> {
        self.skip_whitespace();
        self.expect_byte(b'(', "expected '(' after transfer_native")?;
        let recipient_source = self.take_expression_until(b',')?.to_owned();
        let (recipient, recipient_type) = self.compile_expression(&recipient_source)?;
        if recipient_type != ValueType::Address {
            return Err(format!(
                "transfer recipient has type {}, expected address",
                recipient_type.name()
            ));
        }
        let amount_source = self.take_expression_until(b')')?.to_owned();
        let (amount, amount_type) = self.compile_expression(&amount_source)?;
        if amount_type != ValueType::U256 {
            return Err(format!(
                "transfer amount has type {}, expected u256",
                amount_type.name()
            ));
        }
        self.skip_whitespace();
        self.expect_byte(b';', "expected ';' after transfer_native")?;
        Ok(Statement::Transfer { recipient, amount })
    }

    fn parse_call(&mut self) -> Result<Statement, String> {
        self.skip_whitespace();
        self.expect_byte(b'(', "expected '(' after call_contract")?;
        let target_source = self.take_expression_until(b',')?.to_owned();
        let (target, target_type) = self.compile_expression(&target_source)?;
        if target_type != ValueType::Address {
            return Err(format!(
                "contract call target has type {}, expected address",
                target_type.name()
            ));
        }
        let selector_source = self.take_expression_until(b',')?.to_owned();
        let (selector, selector_type) = self.compile_expression(&selector_source)?;
        if selector_type != ValueType::Bytes32 {
            return Err(format!(
                "contract call selector has type {}, expected bytes32",
                selector_type.name()
            ));
        }
        let value_source = self.take_expression_until(b')')?.to_owned();
        let (value, value_type) = self.compile_expression(&value_source)?;
        if value_type != ValueType::U256 {
            return Err(format!(
                "contract call value has type {}, expected u256",
                value_type.name()
            ));
        }
        self.skip_whitespace();
        self.expect_byte(b';', "expected ';' after call_contract")?;
        Ok(Statement::Call {
            target,
            selector,
            value,
        })
    }

    fn parse_if(&mut self, depth: usize) -> Result<Statement, String> {
        let condition_source = self.take_expression_until(b'{')?.to_owned();
        let (condition, condition_type) = self.compile_expression(&condition_source)?;
        if condition_type != ValueType::Bool {
            return Err(format!(
                "if condition has type {}, expected bool",
                condition_type.name()
            ));
        }

        let inherited_local_count = self.local_names.len();
        let then_branch = nonempty_branch(self.parse_block(true, depth + 1, false)?);
        self.local_names.truncate(inherited_local_count);
        self.local_types.truncate(inherited_local_count);
        self.local_mutability.truncate(inherited_local_count);

        self.skip_whitespace();
        if !self.consume_keyword("else") {
            return Ok(Statement::If {
                condition,
                then_branch,
                else_branch: nonempty_branch(Vec::new()),
            });
        }
        self.skip_whitespace();
        self.expect_byte(b'{', "expected '{' after else")?;
        let else_branch = nonempty_branch(self.parse_block(true, depth + 1, false)?);
        self.local_names.truncate(inherited_local_count);
        self.local_types.truncate(inherited_local_count);
        self.local_mutability.truncate(inherited_local_count);
        Ok(Statement::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn parse_repeat(&mut self, depth: usize) -> Result<Statement, String> {
        let count_source = self.take_expression_until(b'{')?.to_owned();
        let (count, count_type) = self.compile_expression(&count_source)?;
        if count_type != ValueType::U64 {
            return Err(format!(
                "repeat count has type {}, expected u64",
                count_type.name()
            ));
        }
        let inherited_local_count = self.local_names.len();
        let body = self.parse_block(true, depth + 1, false)?;
        self.local_names.truncate(inherited_local_count);
        self.local_types.truncate(inherited_local_count);
        self.local_mutability.truncate(inherited_local_count);
        Ok(Statement::Repeat { count, body })
    }

    fn compile_expression(&self, source: &str) -> Result<(Vec<Instruction>, ValueType), String> {
        ExpressionParser::new(
            source,
            &self.parameter_names,
            self.parameter_types,
            &self.local_names,
            &self.local_types,
            ExpressionEnvironment {
                constants: self.constants,
                storage: self.storage,
                maps: self.maps,
            },
        )?
        .parse()
    }

    fn take_expression_until(&mut self, terminator: u8) -> Result<&str, String> {
        self.skip_whitespace();
        let start = self.position;
        let mut parentheses = 0usize;
        while let Some(byte) = self.peek_byte() {
            match byte {
                value if value == terminator && parentheses == 0 => {
                    let expression = self.source[start..self.position].trim();
                    self.position += 1;
                    if expression.is_empty() {
                        return Err("expected expression".to_string());
                    }
                    return Ok(expression);
                }
                b'(' => parentheses += 1,
                b')' => {
                    parentheses = parentheses
                        .checked_sub(1)
                        .ok_or_else(|| "unmatched ')' in expression".to_string())?;
                }
                _ => {}
            }
            self.position += 1;
        }
        Err(format!(
            "expected '{}' after expression",
            terminator as char
        ))
    }

    fn parse_identifier(&mut self) -> Result<String, String> {
        self.skip_whitespace();
        let start = self.position;
        let Some(first) = self.peek_byte() else {
            return Err("expected identifier".to_string());
        };
        if !first.is_ascii_alphabetic() && first != b'_' {
            return Err("expected identifier".to_string());
        }
        self.position += 1;
        while matches!(self.peek_byte(), Some(byte) if byte.is_ascii_alphanumeric() || byte == b'_')
        {
            self.position += 1;
        }
        Ok(self.source[start..self.position].to_string())
    }

    fn consume_keyword(&mut self, keyword: &str) -> bool {
        let remaining = &self.source[self.position..];
        if !remaining.starts_with(keyword) {
            return false;
        }
        let boundary = remaining.as_bytes().get(keyword.len()).copied();
        if matches!(boundary, Some(byte) if byte.is_ascii_alphanumeric() || byte == b'_') {
            return false;
        }
        self.position += keyword.len();
        true
    }

    fn consume_self_prefix(&mut self) -> bool {
        if self.source[self.position..].starts_with("self.") {
            self.position += "self.".len();
            true
        } else {
            false
        }
    }

    fn consume_byte(&mut self, expected: u8) -> bool {
        if self.peek_byte() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_byte(&mut self, expected: u8, message: &str) -> Result<(), String> {
        if self.consume_byte(expected) {
            Ok(())
        } else {
            Err(message.to_string())
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek_byte(), Some(byte) if byte.is_ascii_whitespace()) {
            self.position += 1;
        }
    }

    fn peek_byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.position).copied()
    }
}

fn parse_return(
    function: &lithic_syntax::FuncDecl,
    return_type: ValueType,
    parameter_types: &[ValueType],
    constants: &[CompiledConstant],
    storage: &[StorageField],
    maps: &[MapField],
) -> Result<ReturnValue, String> {
    let body = function.body_src.trim();
    let value = body
        .strip_prefix("return")
        .and_then(|rest| rest.trim().strip_suffix(';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "expected exactly 'return <constant-or-parameter>;'".to_string())?;

    if let Some((index, _)) = function
        .params
        .iter()
        .enumerate()
        .find(|(_, parameter)| parameter.name == value)
    {
        if parameter_types[index] != return_type {
            return Err(format!(
                "return parameter '{}' has type {}, expected {}",
                value,
                parameter_types[index].name(),
                return_type.name()
            ));
        }
        return Ok(ReturnValue::Parameter(index as u16));
    }
    if let Some(constant) = constants.iter().find(|constant| constant.name == value) {
        if constant.value_type != return_type {
            return Err(format!(
                "constant '{}' has type {}, expected {}",
                value,
                constant.value_type.name(),
                return_type.name()
            ));
        }
        return Ok(ReturnValue::Constant(constant.word));
    }
    if let Ok(constant) = parse_constant(value, return_type) {
        return Ok(ReturnValue::Constant(constant));
    }
    let (instructions, expression_type) = ExpressionParser::new(
        value,
        &function
            .params
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        parameter_types,
        &[],
        &[],
        ExpressionEnvironment {
            constants,
            storage,
            maps,
        },
    )?
    .parse()?;
    if expression_type != return_type {
        return Err(format!(
            "expression has type {}, expected {}",
            expression_type.name(),
            return_type.name()
        ));
    }
    Ok(ReturnValue::Expression(instructions))
}

fn parse_constant(value: &str, value_type: ValueType) -> Result<[u8; 32], String> {
    match value_type {
        ValueType::String => {
            Err("string literals are not yet supported; pass a string parameter".to_string())
        }
        ValueType::Bool => match value {
            "true" => Ok(word_from_u64(1)),
            "false" => Ok([0; 32]),
            _ => Err("bool return must be true or false".to_string()),
        },
        ValueType::Address => parse_fixed_hex(value, 20),
        ValueType::Bytes32 => parse_fixed_hex(value, 32),
        ValueType::U64 => value
            .parse::<u64>()
            .map(word_from_u64)
            .map_err(|_| "u64 return must be an unsigned decimal literal".to_string()),
        ValueType::U256 => parse_u256_decimal(value),
    }
}

fn compile_constant_value(value: &str, value_type: ValueType) -> Result<[u8; 32], String> {
    if value_type == ValueType::Bytes32 {
        if let Some(argument) = value
            .strip_prefix("keccak256(")
            .and_then(|rest| rest.strip_suffix(')'))
        {
            let text = argument
                .trim()
                .strip_prefix('"')
                .and_then(|rest| rest.strip_suffix('"'))
                .ok_or_else(|| "keccak256 constant requires one string literal".to_string())?;
            if text.contains('"') || text.contains('\\') {
                return Err(
                    "keccak256 constant string cannot contain quotes or escape sequences"
                        .to_string(),
                );
            }
            let digest = Keccak256::digest(text.as_bytes());
            let mut word = [0; 32];
            word.copy_from_slice(&digest);
            return Ok(word);
        }
    }
    parse_constant(value, value_type)
}

fn parse_fixed_hex(value: &str, width: usize) -> Result<[u8; 32], String> {
    let encoded = value
        .strip_prefix("0x")
        .ok_or_else(|| "hex constant must start with 0x".to_string())?;
    if encoded.len() != width * 2 {
        return Err(format!("hex constant must contain exactly {width} bytes"));
    }
    let decoded = hex::decode(encoded).map_err(|_| "invalid hex constant".to_string())?;
    let mut word = [0u8; 32];
    word[32 - width..].copy_from_slice(&decoded);
    Ok(word)
}

fn parse_u256_decimal(value: &str) -> Result<[u8; 32], String> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("u256 return must be an unsigned decimal literal".to_string());
    }
    let mut word = [0u8; 32];
    for digit in value.bytes().map(|byte| byte - b'0') {
        let mut carry = digit as u16;
        for byte in word.iter_mut().rev() {
            let next = (*byte as u16) * 10 + carry;
            *byte = next as u8;
            carry = next >> 8;
        }
        if carry != 0 {
            return Err("u256 literal exceeds 256 bits".to_string());
        }
    }
    Ok(word)
}

fn word_from_u64(value: u64) -> [u8; 32] {
    let mut word = [0u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ExprToken {
    Ident(String),
    Int(String),
    Bool(bool),
    LParen,
    RParen,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Star,
    Slash,
    EqEq,
    Lt,
    Gte,
    Eof,
}

struct ExpressionParser<'a> {
    tokens: Vec<ExprToken>,
    position: usize,
    depth: usize,
    parameter_names: &'a [&'a str],
    parameter_types: &'a [ValueType],
    local_names: &'a [String],
    local_types: &'a [ValueType],
    environment: ExpressionEnvironment<'a>,
}

#[derive(Clone, Copy)]
struct ExpressionEnvironment<'a> {
    constants: &'a [CompiledConstant],
    storage: &'a [StorageField],
    maps: &'a [MapField],
}

impl<'a> ExpressionParser<'a> {
    fn new(
        source: &str,
        parameter_names: &'a [&'a str],
        parameter_types: &'a [ValueType],
        local_names: &'a [String],
        local_types: &'a [ValueType],
        environment: ExpressionEnvironment<'a>,
    ) -> Result<Self, String> {
        Ok(Self {
            tokens: lex_expression(source)?,
            position: 0,
            depth: 0,
            parameter_names,
            parameter_types,
            local_names,
            local_types,
            environment,
        })
    }

    fn parse(mut self) -> Result<(Vec<Instruction>, ValueType), String> {
        let result = self.parse_equality()?;
        if self.current() != &ExprToken::Eof {
            return Err("unexpected token after return expression".to_string());
        }
        Ok(result)
    }

    fn parse_equality(&mut self) -> Result<(Vec<Instruction>, ValueType), String> {
        let mut left = self.parse_comparison()?;
        while self.eat(&ExprToken::EqEq) {
            let right = self.parse_comparison()?;
            if left.1 != right.1 {
                return Err("equality operands must have the same type".to_string());
            }
            left.0.extend(right.0);
            left.0.push(Instruction::Eq);
            left.1 = ValueType::Bool;
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<(Vec<Instruction>, ValueType), String> {
        let mut left = self.parse_additive()?;
        loop {
            let gte = if self.eat(&ExprToken::Gte) {
                true
            } else if self.eat(&ExprToken::Lt) {
                false
            } else {
                break;
            };
            let right = self.parse_additive()?;
            if gte {
                let (mut left_code, mut right_code, value_type) =
                    coerce_numeric_pair(left, right, "comparison")?;
                if value_type != ValueType::U256 {
                    return Err("'>=' currently requires u256 operands".to_string());
                }
                left_code.append(&mut right_code);
                left_code.push(Instruction::GteU256);
                left = (left_code, ValueType::Bool);
            } else {
                require_u64_pair(left.1, right.1, "comparison")?;
                left.0.extend(right.0);
                left.0.push(Instruction::LtU64);
                left.1 = ValueType::Bool;
            }
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<(Vec<Instruction>, ValueType), String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            let add = if self.eat(&ExprToken::Plus) {
                Some(true)
            } else if self.eat(&ExprToken::Minus) {
                Some(false)
            } else {
                None
            };
            let Some(add) = add else {
                break;
            };
            let right = self.parse_multiplicative()?;
            let (mut left_code, mut right_code, value_type) =
                coerce_numeric_pair(left, right, "arithmetic")?;
            left_code.append(&mut right_code);
            left_code.push(match (value_type, add) {
                (ValueType::U64, true) => Instruction::AddU64,
                (ValueType::U64, false) => Instruction::SubU64,
                (ValueType::U256, true) => Instruction::AddU256,
                (ValueType::U256, false) => Instruction::SubU256,
                _ => unreachable!(),
            });
            left = (left_code, value_type);
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<(Vec<Instruction>, ValueType), String> {
        let mut left = self.parse_primary()?;
        loop {
            let instruction = if self.eat(&ExprToken::Star) {
                Some(Instruction::MulU64)
            } else if self.eat(&ExprToken::Slash) {
                Some(Instruction::DivU64)
            } else {
                None
            };
            let Some(instruction) = instruction else {
                break;
            };
            let right = self.parse_primary()?;
            require_u64_pair(left.1, right.1, "arithmetic")?;
            left.0.extend(right.0);
            left.0.push(instruction);
            left.1 = ValueType::U64;
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<(Vec<Instruction>, ValueType), String> {
        match self.bump() {
            ExprToken::Int(value) => {
                let value = value
                    .parse::<u64>()
                    .map_err(|_| "expression integer exceeds u64".to_string())?;
                Ok((
                    vec![Instruction::Constant(ValueType::U64, word_from_u64(value))],
                    ValueType::U64,
                ))
            }
            ExprToken::Bool(value) => Ok((
                vec![Instruction::Constant(
                    ValueType::Bool,
                    word_from_u64(u64::from(value)),
                )],
                ValueType::Bool,
            )),
            ExprToken::Ident(name) => {
                if let Some((instruction, value_type)) = match name.as_str() {
                    "msg.sender" => Some((Instruction::MessageSender, ValueType::Address)),
                    "msg.value" => Some((Instruction::MessageValue, ValueType::U256)),
                    "block.height" => Some((Instruction::BlockHeight, ValueType::U64)),
                    "block.timestamp" => Some((Instruction::BlockTimestamp, ValueType::U64)),
                    "chain.id" => Some((Instruction::ChainId, ValueType::U64)),
                    _ => None,
                } {
                    return Ok((vec![instruction], value_type));
                }
                if let Some(field_name) = name.strip_prefix("self.") {
                    if let Some(index) = self
                        .environment
                        .storage
                        .iter()
                        .position(|field| field.name == field_name)
                    {
                        return Ok((
                            vec![Instruction::Storage(index as u16)],
                            self.environment.storage[index].value_type,
                        ));
                    }
                    if let Some(index) = self
                        .environment
                        .maps
                        .iter()
                        .position(|map| map.name == field_name)
                    {
                        let map = &self.environment.maps[index];
                        let mut instructions = Vec::new();
                        for key_type in &map.key_types {
                            if !self.eat(&ExprToken::LBracket) {
                                return Err(format!("map '{field_name}' requires a key"));
                            }
                            let mut key = self.parse_equality()?;
                            if !self.eat(&ExprToken::RBracket) {
                                return Err("expected ']' after map key".to_string());
                            }
                            coerce_expression(&mut key, *key_type)?;
                            instructions.extend(key.0);
                        }
                        if self.current() == &ExprToken::LBracket {
                            return Err(format!("map '{field_name}' received too many keys"));
                        }
                        instructions.push(Instruction::MapStorage(index as u16));
                        return Ok((instructions, map.value_type));
                    }
                    return Err(format!("unknown storage field '{field_name}'"));
                }
                if let Some(index) = self
                    .parameter_names
                    .iter()
                    .position(|parameter| *parameter == name)
                {
                    return Ok((
                        vec![Instruction::Parameter(index as u16)],
                        self.parameter_types[index],
                    ));
                }
                if let Some(index) = self.local_names.iter().position(|local| *local == name) {
                    return Ok((
                        vec![Instruction::Local(index as u16)],
                        self.local_types[index],
                    ));
                }
                if let Some(constant) = self
                    .environment
                    .constants
                    .iter()
                    .find(|value| value.name == name)
                {
                    return Ok((
                        vec![Instruction::Constant(constant.value_type, constant.word)],
                        constant.value_type,
                    ));
                }
                Err(format!("unknown expression identifier '{name}'"))
            }
            ExprToken::LParen => {
                if self.depth >= MAX_EXPRESSION_DEPTH {
                    return Err(format!(
                        "return expression exceeds maximum nesting depth {MAX_EXPRESSION_DEPTH}"
                    ));
                }
                self.depth += 1;
                let expression = self.parse_equality()?;
                self.depth -= 1;
                if !self.eat(&ExprToken::RParen) {
                    return Err("expected ')' in return expression".to_string());
                }
                Ok(expression)
            }
            token => Err(format!("expected expression value, found {token:?}")),
        }
    }

    fn current(&self) -> &ExprToken {
        &self.tokens[self.position]
    }

    fn bump(&mut self) -> ExprToken {
        let token = self.current().clone();
        if token != ExprToken::Eof {
            self.position += 1;
        }
        token
    }

    fn eat(&mut self, expected: &ExprToken) -> bool {
        if self.current() == expected {
            self.bump();
            true
        } else {
            false
        }
    }
}

const MAX_EXPRESSION_SOURCE_BYTES: usize = 65_536;
const MAX_EXPRESSION_TOKENS: usize = 4_096;
const MAX_EXPRESSION_DEPTH: usize = 128;

fn require_u64_pair(left: ValueType, right: ValueType, operation: &str) -> Result<(), String> {
    if left != ValueType::U64 || right != ValueType::U64 {
        return Err(format!("{operation} currently requires two u64 operands"));
    }
    Ok(())
}

fn coerce_expression(
    expression: &mut (Vec<Instruction>, ValueType),
    expected: ValueType,
) -> Result<(), String> {
    if expression.1 == expected {
        return Ok(());
    }
    if expected == ValueType::U256
        && expression.1 == ValueType::U64
        && matches!(
            expression.0.as_slice(),
            [Instruction::Constant(ValueType::U64, _)]
        )
    {
        if let Instruction::Constant(value_type, _) = &mut expression.0[0] {
            *value_type = ValueType::U256;
        }
        expression.1 = ValueType::U256;
        return Ok(());
    }
    Err(format!(
        "expression has type {}, expected {}",
        expression.1.name(),
        expected.name()
    ))
}

fn coerce_numeric_pair(
    mut left: (Vec<Instruction>, ValueType),
    mut right: (Vec<Instruction>, ValueType),
    operation: &str,
) -> Result<(Vec<Instruction>, Vec<Instruction>, ValueType), String> {
    if left.1 == right.1 && matches!(left.1, ValueType::U64 | ValueType::U256) {
        return Ok((left.0, right.0, left.1));
    }
    if left.1 == ValueType::U256 {
        coerce_expression(&mut right, ValueType::U256)?;
        return Ok((left.0, right.0, ValueType::U256));
    }
    if right.1 == ValueType::U256 {
        coerce_expression(&mut left, ValueType::U256)?;
        return Ok((left.0, right.0, ValueType::U256));
    }
    Err(format!(
        "{operation} requires matching u64 or u256 operands"
    ))
}

fn lex_expression(source: &str) -> Result<Vec<ExprToken>, String> {
    if source.len() > MAX_EXPRESSION_SOURCE_BYTES {
        return Err(format!(
            "return expression exceeds {MAX_EXPRESSION_SOURCE_BYTES} bytes"
        ));
    }
    let bytes = source.as_bytes();
    let mut position = 0usize;
    let mut tokens = Vec::new();
    while position < bytes.len() {
        let byte = bytes[position];
        if byte.is_ascii_whitespace() {
            position += 1;
            continue;
        }
        if byte.is_ascii_digit() {
            let start = position;
            position += 1;
            while position < bytes.len() && bytes[position].is_ascii_digit() {
                position += 1;
            }
            tokens.push(ExprToken::Int(source[start..position].to_string()));
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = position;
            position += 1;
            while position < bytes.len()
                && (bytes[position].is_ascii_alphanumeric() || bytes[position] == b'_')
            {
                position += 1;
            }
            if bytes.get(position) == Some(&b'.') {
                position += 1;
                let Some(first) = bytes.get(position) else {
                    return Err("expected identifier after '.'".to_string());
                };
                if !first.is_ascii_alphabetic() && *first != b'_' {
                    return Err("expected identifier after '.'".to_string());
                }
                position += 1;
                while position < bytes.len()
                    && (bytes[position].is_ascii_alphanumeric() || bytes[position] == b'_')
                {
                    position += 1;
                }
            }
            let name = &source[start..position];
            tokens.push(match name {
                "true" => ExprToken::Bool(true),
                "false" => ExprToken::Bool(false),
                _ => ExprToken::Ident(name.to_string()),
            });
            continue;
        }
        let token = match byte {
            b'(' => ExprToken::LParen,
            b')' => ExprToken::RParen,
            b'[' => ExprToken::LBracket,
            b']' => ExprToken::RBracket,
            b'+' => ExprToken::Plus,
            b'-' => ExprToken::Minus,
            b'*' => ExprToken::Star,
            b'/' => ExprToken::Slash,
            b'<' => ExprToken::Lt,
            b'>' if bytes.get(position + 1) == Some(&b'=') => {
                position += 1;
                ExprToken::Gte
            }
            b'=' if bytes.get(position + 1) == Some(&b'=') => {
                position += 1;
                ExprToken::EqEq
            }
            _ => return Err(format!("unsupported expression byte 0x{byte:02x}")),
        };
        tokens.push(token);
        position += 1;
    }
    if tokens.len() > MAX_EXPRESSION_TOKENS {
        return Err(format!(
            "return expression exceeds {MAX_EXPRESSION_TOKENS} tokens"
        ));
    }
    tokens.push(ExprToken::Eof);
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lithovm::{ExecutionContext, Storage, Vm, BASE_CALL_GAS, PARAMETER_GAS};

    fn word_from_bool(value: bool) -> [u8; 32] {
        let mut word = [0; 32];
        word[31] = u8::from(value);
        word
    }

    #[test]
    fn compiler_and_native_runtime_execute_the_same_artifact() {
        let artifact = compile(
            "contract C { pub fn answer() -> u64 { return 42; } pub fn echo(value: u64) -> u64 { return value; } }",
        )
        .unwrap();
        assert_eq!(artifact.target, TARGET);
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        assert_eq!(
            &vm.execute(&bytes, "answer", &[], BASE_CALL_GAS)
                .unwrap()
                .return_value[24..],
            &42u64.to_be_bytes()
        );
        assert_eq!(
            vm.execute(
                &bytes,
                "echo",
                &[word_from_u64(9005)],
                BASE_CALL_GAS + PARAMETER_GAS,
            )
            .unwrap()
            .return_value,
            word_from_u64(9005)
        );
    }

    #[test]
    fn artifact_exposes_canonical_verification_metadata() {
        let source =
            "contract C { pub fn transfer(to: address, amount: u256) -> bool { return true; } }";
        let artifact = compile(source).unwrap();

        assert_eq!(artifact.artifact_version, 1);
        assert_eq!(artifact.compiler, "lithc");
        assert_eq!(artifact.compiler_version, "0.2.0");
        assert_eq!(
            artifact.source_hash,
            "0x32ad261ca2d8fa0510478c6d4559a9f8416cc38960a48dc78b7d91c72f6fcc73"
        );
        assert_eq!(
            artifact.code_hash,
            "0xe73e57c4e59ec327bccb0480e3d1eaefb3403e207c740c1a388745a10bb2fb91"
        );
        assert_eq!(artifact.entrypoints.len(), 1);
        assert_eq!(artifact.entrypoints[0].name, "transfer");
        assert_eq!(artifact.entrypoints[0].signature, "transfer(address,u256)");
        assert_eq!(
            artifact.entrypoints[0].selector,
            "0xf61367304e4e32065cad538b10a44bb599f78e0771530108572d225c74122c1f"
        );
        assert_eq!(compile(source).unwrap(), artifact);
    }

    #[test]
    fn output_is_deterministic_and_fail_closed() {
        let source = "contract C { pub fn answer() -> u64 { return 42; } }";
        assert_eq!(compile(source).unwrap(), compile(source).unwrap());
        for unsupported in [
            "contract C { state { values: vec<u64>; } pub fn x() -> u64 { return 1; } }",
            "contract C { event Seen { values: map<address, u64> } pub fn x() -> u64 { return 1; } }",
            "contract C { pub fn x() -> u64 { return call(); } }",
            "contract C { pub async fn x() -> u64 { return 1; } }",
        ] {
            assert!(compile(unsupported).is_err(), "compiled {unsupported}");
        }
    }

    #[test]
    fn compiles_precedence_and_executes_checked_u64_expressions() {
        let artifact = compile(
            "contract C { pub fn calculate(value: u64) -> u64 { return value + 2 * 3; } pub fn small(value: u64) -> bool { return value < 10; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        let result = vm
            .execute(&bytes, "calculate", &[word_from_u64(36)], 100)
            .unwrap();
        assert_eq!(result.return_value, word_from_u64(42));
        assert_eq!(
            vm.execute(&bytes, "small", &[word_from_u64(9)], 100)
                .unwrap()
                .return_value,
            word_from_u64(1)
        );
    }

    #[test]
    fn rejects_expression_type_errors_and_runtime_faults() {
        assert!(
            compile("contract C { pub fn bad(flag: bool) -> u64 { return flag + 1; } }")
                .unwrap_err()
                .to_string()
                .contains("requires matching u64 or u256 operands")
        );
        assert!(
            compile("contract C { pub fn bad(value: u64) -> bool { return value + 1; } }")
                .unwrap_err()
                .to_string()
                .contains("expression has type u64, expected bool")
        );

        let artifact = compile(
            "contract C { pub fn divide(value: u64, divisor: u64) -> u64 { return value / divisor; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        assert!(Vm::default()
            .execute(
                &bytes,
                "divide",
                &[word_from_u64(42), word_from_u64(0)],
                100,
            )
            .is_err());
    }

    #[test]
    fn rejects_excessive_expression_depth_and_size() {
        let nested = format!(
            "contract C {{ pub fn x() -> u64 {{ return {}1{}; }} }}",
            "(".repeat(MAX_EXPRESSION_DEPTH + 1),
            ")".repeat(MAX_EXPRESSION_DEPTH + 1)
        );
        assert!(compile(&nested)
            .unwrap_err()
            .to_string()
            .contains("maximum nesting depth"));

        let oversized = format!(
            "contract C {{ pub fn x() -> u64 {{ return {}; }} }}",
            "1+".repeat(MAX_EXPRESSION_TOKENS) + "1"
        );
        assert!(compile(&oversized)
            .unwrap_err()
            .to_string()
            .contains("tokens"));
    }

    #[test]
    fn compiles_locals_and_executes_only_the_selected_branch() {
        let artifact = compile(
            "contract C { pub fn choose(value: u64, limit: u64) -> u64 { let doubled: u64 = value * 2; if doubled < limit { return doubled; } else { let fallback = limit + 1; return fallback; } } }",
        )
        .unwrap();
        assert_eq!(artifact.target, "lithovm-native-v11");
        assert_eq!(artifact.bytecode_version, 11);
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();

        let selected = vm
            .execute(
                &bytes,
                "choose",
                &[word_from_u64(4), word_from_u64(10)],
                100,
            )
            .unwrap();
        assert_eq!(selected.return_value, word_from_u64(8));

        let fallback = vm
            .execute(
                &bytes,
                "choose",
                &[word_from_u64(6), word_from_u64(10)],
                100,
            )
            .unwrap();
        assert_eq!(fallback.return_value, word_from_u64(11));
        assert!(fallback.gas_used > selected.gas_used);

        let lazy = compile(
            "contract C { pub fn safe(flag: bool) -> u64 { if flag { return 7; } else { return 1 / 0; } } }",
        )
        .unwrap();
        let lazy_bytes = hex::decode(&lazy.bytecode[2..]).unwrap();
        assert_eq!(
            vm.execute(&lazy_bytes, "safe", &[word_from_u64(1)], 100)
                .unwrap()
                .return_value,
            word_from_u64(7)
        );
        assert!(vm
            .execute(&lazy_bytes, "safe", &[word_from_u64(0)], 100)
            .is_err());
    }

    #[test]
    fn rejects_invalid_local_and_control_flow_bodies() {
        for (source, expected) in [
            (
                "contract C { pub fn x(value: u64) -> u64 { let value = 1; return value; } }",
                "duplicate local binding",
            ),
            (
                "contract C { pub fn x(value: u64) -> u64 { if value < 1 { return 1; } } }",
                "function does not return on every path",
            ),
            (
                "contract C { pub fn x(value: u64) -> u64 { if value { return 1; } else { return 2; } } }",
                "if condition has type u64",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn scalar_storage_commits_atomically_and_rolls_back_failures() {
        let artifact = compile(
            "contract Counter { state { count: u64; } pub fn get() -> u64 { return self.count; } pub fn set(value: u64) -> u64 { self.count = value; return self.count; } pub fn fail(value: u64) -> u64 { self.count = value; self.count = value / 0; return self.count; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        let mut storage = Storage::default();

        assert!(vm.execute(&bytes, "get", &[], 100).is_err());
        assert_eq!(
            vm.execute_with_storage(&bytes, "get", &[], 100, &mut storage)
                .unwrap()
                .return_value,
            word_from_u64(0)
        );
        assert_eq!(
            vm.execute_with_storage(&bytes, "set", &[word_from_u64(41)], 100, &mut storage,)
                .unwrap()
                .return_value,
            word_from_u64(41)
        );
        assert_eq!(storage.get("count"), Some(&word_from_u64(41)));

        assert!(vm
            .execute_with_storage(
                &bytes,
                "set",
                &[word_from_u64(77)],
                BASE_CALL_GAS + PARAMETER_GAS + 2,
                &mut storage,
            )
            .is_err());
        assert_eq!(storage.get("count"), Some(&word_from_u64(41)));

        assert!(vm
            .execute_with_storage(&bytes, "fail", &[word_from_u64(99)], 100, &mut storage,)
            .is_err());
        assert_eq!(storage.get("count"), Some(&word_from_u64(41)));
    }

    #[test]
    fn rejects_unknown_or_mistyped_storage_access() {
        for (source, expected) in [
            (
                "contract C { state { value: u64; } pub fn x() -> u64 { return self.missing; } }",
                "unknown storage field 'missing'",
            ),
            (
                "contract C { state { value: bool; } pub fn x(input: u64) -> bool { self.value = input; return self.value; } }",
                "storage field 'value' has type bool",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn typed_map_access_fails_closed() {
        for (source, expected) in [
            (
                "contract C { state { balances: map<address, u256>; } pub fn x() -> u256 { return self.balances[1]; } }",
                "expression has type u64, expected address",
            ),
            (
                "contract C { state { allowances: map<address, map<address, u256>>; } pub fn x(owner: address) -> u256 { return self.allowances[owner]; } }",
                "map 'allowances' requires a key",
            ),
            (
                "contract C { state { balances: map<address, u256>; } pub fn x(a: address, b: address) -> u256 { return self.balances[a][b]; } }",
                "map 'balances' received too many keys",
            ),
            (
                "contract C { state { balances: map<address, u256>; } pub fn x(a: address) -> bool { self.balances[a] = true; return true; } }",
                "map 'balances' has value type u256, expression has type bool",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn compiles_and_executes_explicit_host_context() {
        let artifact = compile(
            "contract Context { pub fn sender() -> address { return msg.sender; } pub fn value() -> u256 { return msg.value; } pub fn height() -> u64 { return block.height; } pub fn timestamp() -> u64 { return block.timestamp; } pub fn chain() -> u64 { return chain.id; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let mut caller = [0; 32];
        caller[12..].copy_from_slice(&[7; 20]);
        let context = ExecutionContext {
            caller,
            value: word_from_u64(25),
            block_height: 91,
            block_timestamp: 1_725_000_000,
            chain_id: 9005,
            contract_balance: [0; 32],
            call_depth: 0,
        };
        let vm = Vm::default();

        assert!(vm.execute(&bytes, "sender", &[], 100).is_err());
        for (name, expected) in [
            ("sender", caller),
            ("value", word_from_u64(25)),
            ("height", word_from_u64(91)),
            ("timestamp", word_from_u64(1_725_000_000)),
            ("chain", word_from_u64(9005)),
        ] {
            assert_eq!(
                vm.execute_with_context(&bytes, name, &[], 100, &context)
                    .unwrap()
                    .return_value,
                expected
            );
        }
    }

    #[test]
    fn context_storage_execution_remains_transactional() {
        let artifact = compile(
            "contract ContextState { state { last: u64; } pub fn record() -> u64 { self.last = block.timestamp; return self.last; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        let mut storage = Storage::default();
        let context = ExecutionContext {
            block_timestamp: 1234,
            chain_id: 9005,
            ..ExecutionContext::default()
        };

        assert!(vm
            .execute_with_storage(&bytes, "record", &[], 100, &mut storage)
            .is_err());
        assert_eq!(storage.get("last"), None);
        assert_eq!(
            vm.execute_with_storage_and_context(
                &bytes,
                "record",
                &[],
                100,
                &mut storage,
                &context,
            )
            .unwrap()
            .return_value,
            word_from_u64(1234)
        );
        assert_eq!(storage.get("last"), Some(&word_from_u64(1234)));
    }

    #[test]
    fn typed_events_are_emitted_in_order_after_success() {
        let artifact = compile(
            "contract Emitter { event Changed { account: address, value: u64 } pub fn change(account: address, value: u64) -> u64 { emit Changed { account: account, value: value }; return value; } }",
        )
        .unwrap();
        assert_eq!(artifact.abi[0]["type"], "event");
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let mut account = [0; 32];
        account[12..].copy_from_slice(&[9; 20]);
        let result = Vm::default()
            .execute(&bytes, "change", &[account, word_from_u64(42)], 100)
            .unwrap();

        assert_eq!(result.return_value, word_from_u64(42));
        assert_eq!(result.events.len(), 1);
        assert_eq!(result.events[0].name, "Changed");
        assert_eq!(result.events[0].fields[0].0, "account");
        assert_eq!(result.events[0].fields[0].2, account);
        assert_eq!(result.events[0].fields[1].2, word_from_u64(42));
    }

    #[test]
    fn event_schema_and_emissions_fail_closed() {
        for (source, expected) in [
            (
                "contract C { event Seen { value: u64 } pub fn x() -> u64 { emit Missing { value: 1 }; return 1; } }",
                "unknown event 'Missing'",
            ),
            (
                "contract C { event Seen { value: u64 } pub fn x() -> u64 { emit Seen { value: true }; return 1; } }",
                "field 'value' has type u64",
            ),
            (
                "contract C { event Seen { value: u64, account: address } pub fn x() -> u64 { emit Seen { account: 0x0000000000000000000000000000000000000000, value: 1 }; return 1; } }",
                "expected field 'value'",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn native_transfers_are_staged_and_balance_checked() {
        let artifact = compile(
            "contract Payout { pub fn pay(to: address, amount: u256) -> u256 { transfer_native(to, amount); return amount; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let mut recipient = [0; 32];
        recipient[12..].copy_from_slice(&[3; 20]);
        let context = ExecutionContext {
            contract_balance: word_from_u64(100),
            chain_id: 9005,
            ..ExecutionContext::default()
        };
        let vm = Vm::default();

        assert!(vm
            .execute(&bytes, "pay", &[recipient, word_from_u64(40)], 100,)
            .is_err());
        let result = vm
            .execute_with_context(
                &bytes,
                "pay",
                &[recipient, word_from_u64(40)],
                100,
                &context,
            )
            .unwrap();
        assert_eq!(result.transfers.len(), 1);
        assert_eq!(result.transfers[0].recipient, recipient);
        assert_eq!(result.transfers[0].amount, word_from_u64(40));

        assert!(vm
            .execute_with_context(
                &bytes,
                "pay",
                &[recipient, word_from_u64(101)],
                100,
                &context,
            )
            .is_err());

        let twice = compile(
            "contract Payout { pub fn pay(to: address, first: u256, second: u256) -> u256 { transfer_native(to, first); transfer_native(to, second); return second; } }",
        )
        .unwrap();
        let twice_bytes = hex::decode(&twice.bytecode[2..]).unwrap();
        assert!(vm
            .execute_with_context(
                &twice_bytes,
                "pay",
                &[recipient, word_from_u64(60), word_from_u64(41)],
                200,
                &context,
            )
            .is_err());
    }

    #[test]
    fn native_transfer_types_fail_closed() {
        for (source, expected) in [
            (
                "contract C { pub fn x(to: u64, amount: u256) -> u256 { transfer_native(to, amount); return amount; } }",
                "recipient has type u64",
            ),
            (
                "contract C { pub fn x(to: address, amount: u64) -> u64 { transfer_native(to, amount); return amount; } }",
                "amount has type u64",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn contract_calls_are_staged_with_depth_and_balance_limits() {
        let artifact = compile(
            "contract Caller { pub fn invoke(target: address, selector: bytes32, value: u256) -> u256 { call_contract(target, selector, value); return value; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let mut target = [0; 32];
        target[12..].copy_from_slice(&[4; 20]);
        let selector = [7; 32];
        let context = ExecutionContext {
            contract_balance: word_from_u64(100),
            call_depth: 4,
            chain_id: 9005,
            ..ExecutionContext::default()
        };
        let vm = Vm::default();

        let result = vm
            .execute_with_context(
                &bytes,
                "invoke",
                &[target, selector, word_from_u64(30)],
                200,
                &context,
            )
            .unwrap();
        assert_eq!(result.calls.len(), 1);
        assert_eq!(result.calls[0].target, target);
        assert_eq!(result.calls[0].selector, selector);
        assert_eq!(result.calls[0].value, word_from_u64(30));
        assert_eq!(result.calls[0].depth, 5);

        let depth_limited = ExecutionContext {
            call_depth: lithovm::MAX_CALL_DEPTH,
            ..context.clone()
        };
        assert!(vm
            .execute_with_context(
                &bytes,
                "invoke",
                &[target, selector, word_from_u64(1)],
                200,
                &depth_limited,
            )
            .is_err());
        assert!(vm
            .execute_with_context(
                &bytes,
                "invoke",
                &[target, selector, word_from_u64(101)],
                200,
                &context,
            )
            .is_err());

        let mixed = compile(
            "contract Caller { pub fn invoke(recipient: address, target: address, selector: bytes32, transfer_value: u256, call_value: u256) -> u256 { transfer_native(recipient, transfer_value); call_contract(target, selector, call_value); return call_value; } }",
        )
        .unwrap();
        let mixed_bytes = hex::decode(&mixed.bytecode[2..]).unwrap();
        let result = vm
            .execute_with_context(
                &mixed_bytes,
                "invoke",
                &[
                    target,
                    target,
                    selector,
                    word_from_u64(60),
                    word_from_u64(40),
                ],
                300,
                &context,
            )
            .unwrap();
        assert_eq!(result.transfers.len(), 1);
        assert_eq!(result.calls.len(), 1);
        assert!(vm
            .execute_with_context(
                &mixed_bytes,
                "invoke",
                &[
                    target,
                    target,
                    selector,
                    word_from_u64(60),
                    word_from_u64(41),
                ],
                300,
                &context,
            )
            .is_err());
    }

    #[test]
    fn contract_call_types_fail_closed() {
        for (source, expected) in [
            (
                "contract C { pub fn x(target: u64, selector: bytes32, value: u256) -> u256 { call_contract(target, selector, value); return value; } }",
                "target has type u64",
            ),
            (
                "contract C { pub fn x(target: address, selector: u64, value: u256) -> u256 { call_contract(target, selector, value); return value; } }",
                "selector has type u64",
            ),
            (
                "contract C { pub fn x(target: address, selector: bytes32, value: u64) -> u64 { call_contract(target, selector, value); return value; } }",
                "value has type u64",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn mutable_locals_execute_with_checked_assignment() {
        let artifact = compile(
            "contract Counter { pub fn increment(value: u64, enabled: bool) -> u64 { let mut current: u64 = value; if enabled { current = current + 1; return current; } else { return current; } } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        let enabled = vm
            .execute(
                &bytes,
                "increment",
                &[word_from_u64(41), word_from_bool(true)],
                200,
            )
            .unwrap();
        assert_eq!(enabled.return_value, word_from_u64(42));
        let disabled = vm
            .execute(
                &bytes,
                "increment",
                &[word_from_u64(41), word_from_bool(false)],
                200,
            )
            .unwrap();
        assert_eq!(disabled.return_value, word_from_u64(41));
    }

    #[test]
    fn local_assignment_fails_closed() {
        for (source, expected) in [
            (
                "contract C { pub fn x(value: u64) -> u64 { let current = value; current = current + 1; return current; } }",
                "local binding 'current' is immutable",
            ),
            (
                "contract C { pub fn x(value: u64) -> u64 { value = value + 1; return value; } }",
                "parameter 'value' is immutable",
            ),
            (
                "contract C { pub fn x(value: u64, replacement: bool) -> u64 { let mut current = value; current = replacement; return current; } }",
                "local binding 'current' has type u64, expression has type bool",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn bounded_repeat_loops_execute_and_meter_each_iteration() {
        let artifact = compile(
            "contract Counter { pub fn count(iterations: u64) -> u64 { let mut current: u64 = 0; repeat iterations { current = current + 1; } return current; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        assert_eq!(
            vm.execute(&bytes, "count", &[word_from_u64(5)], 200)
                .unwrap()
                .return_value,
            word_from_u64(5)
        );
        assert_eq!(
            vm.execute(&bytes, "count", &[word_from_u64(0)], 200)
                .unwrap()
                .return_value,
            word_from_u64(0)
        );
        assert!(vm
            .execute(
                &bytes,
                "count",
                &[word_from_u64(lithovm::MAX_LOOP_ITERATIONS + 1)],
                10_000,
            )
            .is_err());
        assert!(vm
            .execute(&bytes, "count", &[word_from_u64(5)], 10)
            .is_err());
    }

    #[test]
    fn repeat_loop_types_fail_closed() {
        assert!(compile(
            "contract C { pub fn x(enabled: bool) -> u64 { let mut current = 0; repeat enabled { current = current + 1; } return current; } }"
        )
        .unwrap_err()
        .to_string()
        .contains("repeat count has type bool"));
    }

    #[test]
    fn typed_contract_constants_lower_into_existing_instructions() {
        let artifact = compile(
            "contract Roles { const LIMIT: u64 = 41; const ADMIN_ROLE: bytes32 = keccak256(\"ADMIN_ROLE\"); pub fn limit() -> u64 { return LIMIT + 1; } pub fn role() -> bytes32 { return ADMIN_ROLE; } }",
        )
        .unwrap();
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        assert_eq!(
            vm.execute(&bytes, "limit", &[], 100).unwrap().return_value,
            word_from_u64(42)
        );
        let digest = Keccak256::digest(b"ADMIN_ROLE");
        assert_eq!(
            vm.execute(&bytes, "role", &[], 100).unwrap().return_value,
            digest.as_slice()
        );
    }

    #[test]
    fn invalid_contract_constants_fail_closed() {
        for (source, expected) in [
            (
                "contract C { const BAD: bytes32 = keccak256(ADMIN_ROLE); pub fn x() -> bytes32 { return BAD; } }",
                "keccak256 constant requires one string literal",
            ),
            (
                "contract C { const FLAG: bool = 2; pub fn x() -> bool { return FLAG; } }",
                "bool return must be true or false",
            ),
            (
                "contract C { const FLAG: bool = true; pub fn x() -> u64 { return FLAG; } }",
                "constant 'FLAG' has type bool, expected u64",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }

    #[test]
    fn explicit_failures_roll_back_all_effects_and_allow_recovery() {
        let artifact = compile(
            "contract Guarded { state { value: u64; } event Changed { value: u64 } pub fn update(allowed: bool, recipient: address, target: address, selector: bytes32, amount: u256) -> u64 { self.value = 7; emit Changed { value: self.value }; transfer_native(recipient, amount); call_contract(target, selector, amount); require(allowed); return self.value; } pub fn abort() -> u64 { self.value = 9; revert(); } }",
        )
        .unwrap();
        assert_eq!(artifact.target, "lithovm-native-v11");
        assert_eq!(artifact.bytecode_version, 11);
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let mut account = [0; 32];
        account[12..].copy_from_slice(&[3; 20]);
        let selector = [7; 32];
        let context = ExecutionContext {
            contract_balance: word_from_u64(100),
            chain_id: 700777,
            ..ExecutionContext::default()
        };
        let vm = Vm::default();
        let mut storage = Storage::default();
        storage.set_word("value", word_from_u64(3));

        let denied = vm
            .execute_with_storage_and_context(
                &bytes,
                "update",
                &[
                    word_from_bool(false),
                    account,
                    account,
                    selector,
                    word_from_u64(10),
                ],
                500,
                &mut storage,
                &context,
            )
            .unwrap_err();
        assert!(denied.to_string().contains("require condition failed"));
        assert_eq!(storage.get("value"), Some(&word_from_u64(3)));

        let reverted = vm
            .execute_with_storage_and_context(&bytes, "abort", &[], 100, &mut storage, &context)
            .unwrap_err();
        assert!(reverted.to_string().contains("execution reverted"));
        assert_eq!(storage.get("value"), Some(&word_from_u64(3)));

        let recovered = vm
            .execute_with_storage_and_context(
                &bytes,
                "update",
                &[
                    word_from_bool(true),
                    account,
                    account,
                    selector,
                    word_from_u64(10),
                ],
                500,
                &mut storage,
                &context,
            )
            .unwrap();
        assert_eq!(recovered.return_value, word_from_u64(7));
        assert_eq!(recovered.events.len(), 1);
        assert_eq!(recovered.transfers.len(), 1);
        assert_eq!(recovered.calls.len(), 1);
        assert_eq!(storage.get("value"), Some(&word_from_u64(7)));
    }

    #[test]
    fn lax_lep100_candidate_executes_balances_allowances_and_rollback() {
        let source = include_str!("../../../../sdk/contracts/standards/lax_lep100_v11.lithic");
        let artifact = compile(source).unwrap();
        assert_eq!(artifact.target, "lithovm-native-v11");
        assert_eq!(artifact.bytecode_version, 11);
        let bytes = hex::decode(&artifact.bytecode[2..]).unwrap();
        let vm = Vm::default();
        let mut storage = Storage::default();
        let mut owner = [0; 32];
        owner[12..].copy_from_slice(&[1; 20]);
        let mut recipient = [0; 32];
        recipient[12..].copy_from_slice(&[2; 20]);
        let mut spender = [0; 32];
        spender[12..].copy_from_slice(&[3; 20]);
        let supply = parse_u256_decimal("10000000000000000000000000000").unwrap();

        let owner_context = ExecutionContext {
            caller: owner,
            chain_id: 700_777,
            ..ExecutionContext::default()
        };
        let initialized = vm
            .execute_with_storage_and_context(
                &bytes,
                "initialize",
                &[owner],
                1_000,
                &mut storage,
                &owner_context,
            )
            .unwrap();
        assert_eq!(initialized.events.len(), 1);
        assert_eq!(storage.get("total_supply"), Some(&supply));
        assert_eq!(storage.get_map("balances", &[owner]), Some(&supply));
        assert_eq!(
            vm.execute_with_storage(&bytes, "name", &[], 100, &mut storage)
                .unwrap()
                .return_value,
            parse_fixed_hex(
                "0x4c6974686f73706865726520416c676f726974686d6963000000000000000000",
                32,
            )
            .unwrap()
        );
        assert_eq!(
            vm.execute_with_storage(&bytes, "symbol", &[], 100, &mut storage)
                .unwrap()
                .return_value,
            parse_fixed_hex(
                "0x4c41580000000000000000000000000000000000000000000000000000000000",
                32,
            )
            .unwrap()
        );
        assert_eq!(
            vm.execute_with_storage(&bytes, "decimals", &[], 100, &mut storage)
                .unwrap()
                .return_value,
            word_from_u64(18)
        );

        vm.execute_with_storage_and_context(
            &bytes,
            "transfer",
            &[recipient, word_from_u64(100)],
            1_000,
            &mut storage,
            &owner_context,
        )
        .unwrap();
        vm.execute_with_storage_and_context(
            &bytes,
            "approve",
            &[spender, word_from_u64(50)],
            1_000,
            &mut storage,
            &owner_context,
        )
        .unwrap();
        let spender_context = ExecutionContext {
            caller: spender,
            chain_id: 700_777,
            ..ExecutionContext::default()
        };
        vm.execute_with_storage_and_context(
            &bytes,
            "transfer_from",
            &[owner, recipient, word_from_u64(40)],
            1_000,
            &mut storage,
            &spender_context,
        )
        .unwrap();
        assert_eq!(
            storage.get_map("allowances", &[owner, spender]),
            Some(&word_from_u64(10))
        );
        assert_eq!(
            storage.get_map("balances", &[recipient]),
            Some(&word_from_u64(140))
        );
        let recipient_context = ExecutionContext {
            caller: recipient,
            chain_id: 700_777,
            ..ExecutionContext::default()
        };
        vm.execute_with_storage_and_context(
            &bytes,
            "burn",
            &[word_from_u64(40)],
            1_000,
            &mut storage,
            &recipient_context,
        )
        .unwrap();
        assert_eq!(
            storage.get_map("balances", &[recipient]),
            Some(&word_from_u64(100))
        );
        assert_eq!(
            storage.get("total_supply"),
            Some(&parse_u256_decimal("9999999999999999999999999960").unwrap())
        );

        let before_failure = storage.clone();
        let failure = vm.execute_transactionally(
            &bytes,
            "transfer",
            &[recipient, supply],
            1_000,
            &mut storage,
            &owner_context,
        );
        assert!(matches!(
            failure,
            lithovm::ExecutionOutcome::Failure(lithovm::ExecutionFailure {
                kind: lithovm::FailureKind::Revert,
                ..
            })
        ));
        assert_eq!(storage, before_failure);

        let second_initialize = vm.execute_transactionally(
            &bytes,
            "initialize",
            &[recipient],
            1_000,
            &mut storage,
            &owner_context,
        );
        assert!(matches!(
            second_initialize,
            lithovm::ExecutionOutcome::Failure(lithovm::ExecutionFailure {
                kind: lithovm::FailureKind::Revert,
                ..
            })
        ));
        assert_eq!(storage, before_failure);

        let before_out_of_gas = storage.clone();
        let out_of_gas = vm.execute_transactionally(
            &bytes,
            "transfer",
            &[recipient, word_from_u64(1)],
            50,
            &mut storage,
            &owner_context,
        );
        assert!(matches!(
            out_of_gas,
            lithovm::ExecutionOutcome::Failure(lithovm::ExecutionFailure {
                kind: lithovm::FailureKind::OutOfGas,
                gas_used: 50,
                ..
            })
        ));
        assert_eq!(storage, before_out_of_gas);

        storage.set_map_word("balances", vec![recipient], [0xff; 32]);
        let before_overflow = storage.clone();
        let overflow = vm.execute_transactionally(
            &bytes,
            "transfer",
            &[recipient, word_from_u64(1)],
            1_000,
            &mut storage,
            &owner_context,
        );
        assert!(matches!(
            overflow,
            lithovm::ExecutionOutcome::Failure(lithovm::ExecutionFailure {
                kind: lithovm::FailureKind::Trap,
                ..
            })
        ));
        assert_eq!(storage, before_overflow);
    }

    #[test]
    fn failure_syntax_and_types_fail_closed() {
        for (source, expected) in [
            (
                "contract C { pub fn x(value: u64) -> u64 { require(value); return value; } }",
                "require condition has type u64",
            ),
            (
                "contract C { pub fn x() -> u64 { revert(1); } }",
                "revert does not accept arguments",
            ),
            (
                "contract C { pub fn x() -> u64 { revert(); return 1; } }",
                "unreachable statement",
            ),
        ] {
            assert!(
                compile(source).unwrap_err().to_string().contains(expected),
                "missing '{expected}' for {source}"
            );
        }
    }
}
