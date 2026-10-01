use anyhow::{anyhow, bail, Result};
use sha3::{Digest, Keccak256};
pub mod values;

pub const MAGIC: &[u8; 7] = b"LITHOVM";
pub const LEGACY_VERSION: u8 = 1;
pub const STATEMENT_VERSION: u8 = 2;
pub const STORAGE_VERSION: u8 = 3;
pub const CONTEXT_VERSION: u8 = 4;
pub const EVENT_VERSION: u8 = 5;
pub const TRANSFER_VERSION: u8 = 6;
pub const CALL_VERSION: u8 = 7;
pub const MUTABLE_VERSION: u8 = 8;
pub const REPEAT_VERSION: u8 = 9;
pub const FAILURE_VERSION: u8 = 10;
pub const VERSION: u8 = 11;
/// Dynamic-value candidate; scalar programs continue to encode as v11.
pub const STRING_VERSION: u8 = 12;
pub const SYNC_VERSION: u8 = 13;
pub const CREATE_VERSION: u8 = 14;
pub const MAX_FUNCTIONS: usize = 1024;
pub const MAX_PARAMETERS: usize = 64;
pub const MAX_NAME_BYTES: usize = 255;
pub const MAX_LOCALS: usize = 256;
pub const MAX_STATEMENTS: usize = 4096;
pub const MAX_BLOCK_DEPTH: usize = 64;
pub const MAX_STORAGE_FIELDS: usize = 256;
pub const MAX_EVENTS: usize = 256;
pub const MAX_EVENT_FIELDS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ValueType {
    U64 = 1,
    U256 = 2,
    Bool = 3,
    Address = 4,
    Bytes32 = 5,
    String = 6,
}

impl ValueType {
    pub fn from_byte(value: u8) -> Result<Self> {
        match value {
            1 => Ok(Self::U64),
            2 => Ok(Self::U256),
            3 => Ok(Self::Bool),
            4 => Ok(Self::Address),
            5 => Ok(Self::Bytes32),
            6 => Ok(Self::String),
            _ => bail!("unknown LithoVM value type {value}"),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::U64 => "u64",
            Self::U256 => "u256",
            Self::Bool => "bool",
            Self::Address => "address",
            Self::Bytes32 => "bytes32",
            Self::String => "string",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReturnValue {
    Constant([u8; 32]),
    Parameter(u16),
    Expression(Vec<Instruction>),
    Statements(Vec<Statement>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Statement {
    Create {
        template: Vec<Instruction>,
        salt: Vec<Instruction>,
        initializer: Vec<Instruction>,
        value: Vec<Instruction>,
        arguments: Vec<(ValueType, Vec<Instruction>)>,
    },
    Invoke {
        return_type: ValueType,
        target: Vec<Instruction>,
        selector: Vec<Instruction>,
        value: Vec<Instruction>,
        arguments: Vec<(ValueType, Vec<Instruction>)>,
    },
    Let {
        value_type: ValueType,
        expression: Vec<Instruction>,
    },
    LetMutable {
        value_type: ValueType,
        expression: Vec<Instruction>,
    },
    SetLocal {
        local: u16,
        expression: Vec<Instruction>,
    },
    Repeat {
        count: Vec<Instruction>,
        body: Vec<Statement>,
    },
    Require(Vec<Instruction>),
    Revert,
    Return(Vec<Instruction>),
    Store {
        field: u16,
        expression: Vec<Instruction>,
    },
    MapStore {
        map: u16,
        keys: Vec<Vec<Instruction>>,
        expression: Vec<Instruction>,
    },
    Emit {
        event: u16,
        values: Vec<Vec<Instruction>>,
    },
    Transfer {
        recipient: Vec<Instruction>,
        amount: Vec<Instruction>,
    },
    Call {
        target: Vec<Instruction>,
        selector: Vec<Instruction>,
        value: Vec<Instruction>,
    },
    If {
        condition: Vec<Instruction>,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instruction {
    Constant(ValueType, [u8; 32]),
    Parameter(u16),
    Local(u16),
    Storage(u16),
    MapStorage(u16),
    MessageSender,
    MessageValue,
    BlockHeight,
    BlockTimestamp,
    ChainId,
    AddU64,
    SubU64,
    MulU64,
    DivU64,
    Eq,
    LtU64,
    AddU256,
    SubU256,
    GteU256,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<ValueType>,
    pub return_type: ValueType,
    pub return_value: ReturnValue,
}

/// Returns the canonical native ABI signature used for call selection.
pub fn function_signature(function: &Function) -> String {
    let parameters = function
        .parameters
        .iter()
        .map(|value_type| value_type.name())
        .collect::<Vec<_>>()
        .join(",");
    format!("{}({parameters})", function.name)
}

/// Returns the full Keccak-256 digest of the canonical native ABI signature.
pub fn function_selector(function: &Function) -> [u8; 32] {
    Keccak256::digest(function_signature(function).as_bytes()).into()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorageField {
    pub name: String,
    pub value_type: ValueType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapField {
    pub name: String,
    pub key_types: Vec<ValueType>,
    pub value_type: ValueType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventDefinition {
    pub name: String,
    pub fields: Vec<StorageField>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub storage: Vec<StorageField>,
    pub maps: Vec<MapField>,
    pub events: Vec<EventDefinition>,
    pub functions: Vec<Function>,
}

#[derive(Clone, Copy)]
struct ValidationSchema<'a> {
    storage: &'a [StorageField],
    maps: &'a [MapField],
    events: &'a [EventDefinition],
}

impl Program {
    pub fn bytecode_version(&self) -> u8 {
        if self.functions.iter().any(|f| {
            matches!(&f.return_value,
            ReturnValue::Statements(body) if contains_creation(body))
        }) {
            return CREATE_VERSION;
        }
        if self.functions.iter().any(|f| {
            matches!(&f.return_value,
            ReturnValue::Statements(body) if contains_statement(body, true))
        }) {
            return SYNC_VERSION;
        }
        let strings = self
            .storage
            .iter()
            .any(|f| f.value_type == ValueType::String)
            || self
                .events
                .iter()
                .any(|e| e.fields.iter().any(|f| f.value_type == ValueType::String))
            || self.functions.iter().any(|f| {
                f.return_type == ValueType::String || f.parameters.contains(&ValueType::String)
            });
        if strings {
            STRING_VERSION
        } else {
            VERSION
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        validate_storage(&self.storage)?;
        validate_maps(&self.maps)?;
        validate_storage_layout(&self.storage, &self.maps)?;
        validate_events(&self.events)?;
        validate_functions(&self.functions, &self.storage, &self.maps, &self.events)?;
        validate_call_mode(self)?;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.push(self.bytecode_version());
        push_u16(&mut bytes, self.storage.len())?;
        for field in &self.storage {
            push_u16(&mut bytes, field.name.len())?;
            bytes.extend_from_slice(field.name.as_bytes());
            bytes.push(field.value_type as u8);
        }
        push_u16(&mut bytes, self.maps.len())?;
        for map in &self.maps {
            push_name(&mut bytes, &map.name)?;
            push_u16(&mut bytes, map.key_types.len())?;
            bytes.extend(map.key_types.iter().map(|value| *value as u8));
            bytes.push(map.value_type as u8);
        }
        push_u16(&mut bytes, self.events.len())?;
        for event in &self.events {
            push_name(&mut bytes, &event.name)?;
            push_u16(&mut bytes, event.fields.len())?;
            for field in &event.fields {
                push_name(&mut bytes, &field.name)?;
                bytes.push(field.value_type as u8);
            }
        }
        push_u16(&mut bytes, self.functions.len())?;
        for function in &self.functions {
            push_u16(&mut bytes, function.name.len())?;
            bytes.extend_from_slice(function.name.as_bytes());
            push_u16(&mut bytes, function.parameters.len())?;
            bytes.extend(function.parameters.iter().map(|value| *value as u8));
            bytes.push(function.return_type as u8);
            match function.return_value {
                ReturnValue::Constant(word) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&word);
                }
                ReturnValue::Parameter(index) => {
                    bytes.push(2);
                    bytes.extend_from_slice(&index.to_be_bytes());
                }
                ReturnValue::Expression(ref instructions) => {
                    bytes.push(3);
                    encode_expression(&mut bytes, instructions)?;
                }
                ReturnValue::Statements(ref statements) => {
                    bytes.push(4);
                    encode_statements(&mut bytes, statements)?;
                }
            }
        }
        Ok(bytes)
    }
}

pub fn parse(bytes: &[u8]) -> Result<Program> {
    let mut reader = Reader { bytes, position: 0 };
    if reader.take(MAGIC.len())? != MAGIC {
        bail!("invalid LithoVM bytecode magic");
    }
    let version = reader.byte()?;
    if !(LEGACY_VERSION..=CREATE_VERSION).contains(&version) {
        bail!("unsupported LithoVM bytecode version {version}");
    }
    let storage = if version >= STORAGE_VERSION {
        let field_count = reader.u16()? as usize;
        if field_count > MAX_STORAGE_FIELDS {
            bail!("too many LithoVM storage fields: {field_count}");
        }
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            let name_len = reader.u16()? as usize;
            if name_len == 0 || name_len > MAX_NAME_BYTES {
                bail!("invalid LithoVM storage field name length {name_len}");
            }
            let name = std::str::from_utf8(reader.take(name_len)?)
                .map_err(|_| anyhow!("storage field name is not valid UTF-8"))?
                .to_owned();
            let value_type = ValueType::from_byte(reader.byte()?)?;
            fields.push(StorageField { name, value_type });
        }
        validate_storage(&fields)?;
        fields
    } else {
        Vec::new()
    };
    let maps = if version >= VERSION {
        let map_count = reader.u16()? as usize;
        if map_count > MAX_STORAGE_FIELDS {
            bail!("too many LithoVM map fields: {map_count}");
        }
        let mut maps = Vec::with_capacity(map_count);
        for _ in 0..map_count {
            let name = read_name(&mut reader, "map")?;
            let key_count = reader.u16()? as usize;
            if key_count == 0 || key_count > MAX_PARAMETERS {
                bail!("invalid LithoVM map key count {key_count}");
            }
            let mut key_types = Vec::with_capacity(key_count);
            for _ in 0..key_count {
                key_types.push(ValueType::from_byte(reader.byte()?)?);
            }
            maps.push(MapField {
                name,
                key_types,
                value_type: ValueType::from_byte(reader.byte()?)?,
            });
        }
        validate_maps(&maps)?;
        validate_storage_layout(&storage, &maps)?;
        maps
    } else {
        Vec::new()
    };
    let events = if version >= EVENT_VERSION {
        let event_count = reader.u16()? as usize;
        if event_count > MAX_EVENTS {
            bail!("too many LithoVM events: {event_count}");
        }
        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            let name = read_name(&mut reader, "event")?;
            let field_count = reader.u16()? as usize;
            if field_count > MAX_EVENT_FIELDS {
                bail!("too many fields in LithoVM event '{name}'");
            }
            let mut fields = Vec::with_capacity(field_count);
            for _ in 0..field_count {
                fields.push(StorageField {
                    name: read_name(&mut reader, "event field")?,
                    value_type: ValueType::from_byte(reader.byte()?)?,
                });
            }
            events.push(EventDefinition { name, fields });
        }
        validate_events(&events)?;
        events
    } else {
        Vec::new()
    };
    let schema = ValidationSchema {
        storage: &storage,
        maps: &maps,
        events: &events,
    };
    let function_count = reader.u16()? as usize;
    if function_count == 0 || function_count > MAX_FUNCTIONS {
        bail!("invalid LithoVM function count {function_count}");
    }
    let mut functions = Vec::with_capacity(function_count);
    for _ in 0..function_count {
        let name_len = reader.u16()? as usize;
        if name_len == 0 || name_len > MAX_NAME_BYTES {
            bail!("invalid LithoVM function name length {name_len}");
        }
        let name = std::str::from_utf8(reader.take(name_len)?)
            .map_err(|_| anyhow!("function name is not valid UTF-8"))?
            .to_owned();
        let parameter_count = reader.u16()? as usize;
        if parameter_count > MAX_PARAMETERS {
            bail!("too many LithoVM function parameters: {parameter_count}");
        }
        let mut parameters = Vec::with_capacity(parameter_count);
        for _ in 0..parameter_count {
            parameters.push(ValueType::from_byte(reader.byte()?)?);
        }
        let return_type = ValueType::from_byte(reader.byte()?)?;
        let return_value = match reader.byte()? {
            1 => {
                let mut word = [0u8; 32];
                word.copy_from_slice(reader.take(32)?);
                validate_word(return_type, &word)?;
                ReturnValue::Constant(word)
            }
            2 => {
                let index = reader.u16()?;
                let parameter_type = parameters
                    .get(index as usize)
                    .ok_or_else(|| anyhow!("return parameter index {index} is out of range"))?;
                if *parameter_type != return_type {
                    bail!(
                        "return parameter type {} does not match {}",
                        parameter_type.name(),
                        return_type.name()
                    );
                }
                ReturnValue::Parameter(index)
            }
            3 => {
                let instruction_count = reader.u16()? as usize;
                if instruction_count == 0 || instruction_count > MAX_INSTRUCTIONS {
                    bail!("invalid LithoVM instruction count {instruction_count}");
                }
                let mut instructions = Vec::with_capacity(instruction_count);
                for _ in 0..instruction_count {
                    instructions.push(decode_instruction(&mut reader, version)?);
                }
                validate_expression(&instructions, &parameters, &[], &schema, return_type)?;
                ReturnValue::Expression(instructions)
            }
            4 if version >= STATEMENT_VERSION => {
                let statements = decode_statements(&mut reader, version, 0)?;
                let terminal =
                    validate_statements(&statements, &parameters, return_type, &[], &schema, 0)?;
                if !terminal {
                    bail!("function does not return on every path");
                }
                ReturnValue::Statements(statements)
            }
            opcode => bail!("unknown LithoVM return opcode {opcode}"),
        };
        functions.push(Function {
            name,
            parameters,
            return_type,
            return_value,
        });
    }
    if reader.position != bytes.len() {
        bail!("trailing bytes after LithoVM program");
    }
    validate_functions(&functions, &storage, &maps, &events)?;
    let program = Program {
        storage,
        maps,
        events,
        functions,
    };
    let required = program.bytecode_version();
    if required > VERSION && version < required {
        bail!("program requires LithoVM v{required}");
    }
    validate_call_mode(&program)?;
    Ok(program)
}

fn contains_statement(body: &[Statement], invoke: bool) -> bool {
    body.iter().any(|statement| match statement {
        Statement::Invoke { .. } => invoke,
        Statement::Call { .. } => !invoke,
        Statement::If {
            then_branch,
            else_branch,
            ..
        } => contains_statement(then_branch, invoke) || contains_statement(else_branch, invoke),
        Statement::Repeat { body, .. } => contains_statement(body, invoke),
        _ => false,
    })
}

fn contains_creation(body: &[Statement]) -> bool {
    body.iter().any(|statement| match statement {
        Statement::Create { .. } => true,
        Statement::If {
            then_branch,
            else_branch,
            ..
        } => contains_creation(then_branch) || contains_creation(else_branch),
        Statement::Repeat { body, .. } => contains_creation(body),
        _ => false,
    })
}

fn validate_call_mode(program: &Program) -> Result<()> {
    if program.bytecode_version() >= SYNC_VERSION && program.functions.iter().any(|f|
        matches!(&f.return_value, ReturnValue::Statements(body) if contains_statement(body, false))) {
        bail!("cannot mix synchronous effects and deferred call_contract in one program");
    }
    Ok(())
}

pub fn validate_word(value_type: ValueType, word: &[u8; 32]) -> Result<()> {
    match value_type {
        ValueType::String => bail!("string cannot be represented by a scalar word"),
        ValueType::U64 if word[..24] != [0; 24] => bail!("non-canonical u64 value"),
        ValueType::Bool if word[..31] != [0; 31] || word[31] > 1 => {
            bail!("non-canonical bool value")
        }
        ValueType::Address if word[..12] != [0; 12] => bail!("non-canonical address value"),
        _ => Ok(()),
    }
}

fn validate_storage(storage: &[StorageField]) -> Result<()> {
    if storage.len() > MAX_STORAGE_FIELDS {
        bail!("program exceeds {MAX_STORAGE_FIELDS} storage fields");
    }
    for (index, field) in storage.iter().enumerate() {
        if field.name.is_empty()
            || field.name.len() > MAX_NAME_BYTES
            || !field.name.is_ascii()
            || !is_identifier(&field.name)
        {
            bail!("invalid LithoVM storage field name '{}'", field.name);
        }
        if storage[..index]
            .iter()
            .any(|earlier| earlier.name == field.name)
        {
            bail!("duplicate LithoVM storage field name '{}'", field.name);
        }
    }
    Ok(())
}

fn validate_maps(maps: &[MapField]) -> Result<()> {
    if maps.len() > MAX_STORAGE_FIELDS {
        bail!("program exceeds {MAX_STORAGE_FIELDS} map fields");
    }
    for (index, map) in maps.iter().enumerate() {
        if map.value_type == ValueType::String || map.key_types.contains(&ValueType::String) {
            bail!("string map keys and values are unsupported");
        }
        validate_name(&map.name, "map")?;
        if maps[..index].iter().any(|earlier| earlier.name == map.name) {
            bail!("duplicate LithoVM map field name '{}'", map.name);
        }
        if map.key_types.is_empty() || map.key_types.len() > MAX_PARAMETERS {
            bail!("map '{}' has an invalid key count", map.name);
        }
    }
    Ok(())
}

fn validate_storage_layout(storage: &[StorageField], maps: &[MapField]) -> Result<()> {
    if storage.len().saturating_add(maps.len()) > MAX_STORAGE_FIELDS {
        bail!("program exceeds {MAX_STORAGE_FIELDS} total storage fields");
    }
    for map in maps {
        if storage.iter().any(|field| field.name == map.name) {
            bail!("storage name '{}' is used by a scalar and a map", map.name);
        }
    }
    Ok(())
}

fn validate_events(events: &[EventDefinition]) -> Result<()> {
    if events.len() > MAX_EVENTS {
        bail!("program exceeds {MAX_EVENTS} events");
    }
    for (index, event) in events.iter().enumerate() {
        validate_name(&event.name, "event")?;
        if events[..index]
            .iter()
            .any(|earlier| earlier.name == event.name)
        {
            bail!("duplicate LithoVM event name '{}'", event.name);
        }
        if event.fields.len() > MAX_EVENT_FIELDS {
            bail!("event '{}' exceeds {MAX_EVENT_FIELDS} fields", event.name);
        }
        for (field_index, field) in event.fields.iter().enumerate() {
            validate_name(&field.name, "event field")?;
            if event.fields[..field_index]
                .iter()
                .any(|earlier| earlier.name == field.name)
            {
                bail!("duplicate field '{}' in event '{}'", field.name, event.name);
            }
        }
    }
    Ok(())
}

fn validate_functions(
    functions: &[Function],
    storage: &[StorageField],
    maps: &[MapField],
    events: &[EventDefinition],
) -> Result<()> {
    let schema = ValidationSchema {
        storage,
        maps,
        events,
    };
    if functions.is_empty() || functions.len() > MAX_FUNCTIONS {
        bail!("program must contain between 1 and {MAX_FUNCTIONS} functions");
    }
    for (index, function) in functions.iter().enumerate() {
        if function.name.is_empty()
            || function.name.len() > MAX_NAME_BYTES
            || !function.name.is_ascii()
            || !is_identifier(&function.name)
        {
            bail!("invalid LithoVM function name '{}'", function.name);
        }
        if function.parameters.len() > MAX_PARAMETERS {
            bail!("too many parameters in function '{}'", function.name);
        }
        if functions[..index]
            .iter()
            .any(|earlier| earlier.name == function.name)
        {
            bail!("duplicate LithoVM function name '{}'", function.name);
        }
        match function.return_value {
            ReturnValue::Constant(word) => validate_word(function.return_type, &word)?,
            ReturnValue::Parameter(parameter) => {
                let parameter_type = function
                    .parameters
                    .get(parameter as usize)
                    .ok_or_else(|| anyhow!("return parameter index is out of range"))?;
                if *parameter_type != function.return_type {
                    bail!("return parameter type does not match function return type");
                }
            }
            ReturnValue::Expression(ref instructions) => {
                validate_expression(
                    instructions,
                    &function.parameters,
                    &[],
                    &schema,
                    function.return_type,
                )?;
            }
            ReturnValue::Statements(ref statements) => {
                let terminal = validate_statements(
                    statements,
                    &function.parameters,
                    function.return_type,
                    &[],
                    &schema,
                    0,
                )?;
                if !terminal {
                    bail!("function does not return on every path");
                }
            }
        }
    }
    Ok(())
}

pub const MAX_INSTRUCTIONS: usize = 4096;

fn encode_expression(bytes: &mut Vec<u8>, instructions: &[Instruction]) -> Result<()> {
    push_u16(bytes, instructions.len())?;
    for instruction in instructions {
        encode_instruction(bytes, instruction);
    }
    Ok(())
}

fn encode_statements(bytes: &mut Vec<u8>, statements: &[Statement]) -> Result<()> {
    push_u16(bytes, statements.len())?;
    for statement in statements {
        match statement {
            Statement::Create {
                template,
                salt,
                initializer,
                value,
                arguments,
            } => {
                bytes.push(15);
                for expression in [template, salt, initializer, value] {
                    encode_expression(bytes, expression)?;
                }
                push_u16(bytes, arguments.len())?;
                for (ty, expression) in arguments {
                    bytes.push(*ty as u8);
                    encode_expression(bytes, expression)?;
                }
            }
            Statement::Invoke {
                return_type,
                target,
                selector,
                value,
                arguments,
            } => {
                bytes.push(14);
                bytes.push(*return_type as u8);
                encode_expression(bytes, target)?;
                encode_expression(bytes, selector)?;
                encode_expression(bytes, value)?;
                push_u16(bytes, arguments.len())?;
                for (ty, expression) in arguments {
                    bytes.push(*ty as u8);
                    encode_expression(bytes, expression)?;
                }
            }
            Statement::Let {
                value_type,
                expression,
            } => {
                bytes.push(1);
                bytes.push(*value_type as u8);
                encode_expression(bytes, expression)?;
            }
            Statement::LetMutable {
                value_type,
                expression,
            } => {
                bytes.push(8);
                bytes.push(*value_type as u8);
                encode_expression(bytes, expression)?;
            }
            Statement::SetLocal { local, expression } => {
                bytes.push(9);
                bytes.extend_from_slice(&local.to_be_bytes());
                encode_expression(bytes, expression)?;
            }
            Statement::Repeat { count, body } => {
                bytes.push(10);
                encode_expression(bytes, count)?;
                encode_statements(bytes, body)?;
            }
            Statement::Require(condition) => {
                bytes.push(11);
                encode_expression(bytes, condition)?;
            }
            Statement::Revert => bytes.push(12),
            Statement::Return(expression) => {
                bytes.push(2);
                encode_expression(bytes, expression)?;
            }
            Statement::Store { field, expression } => {
                bytes.push(4);
                bytes.extend_from_slice(&field.to_be_bytes());
                encode_expression(bytes, expression)?;
            }
            Statement::MapStore {
                map,
                keys,
                expression,
            } => {
                bytes.push(13);
                bytes.extend_from_slice(&map.to_be_bytes());
                push_u16(bytes, keys.len())?;
                for key in keys {
                    encode_expression(bytes, key)?;
                }
                encode_expression(bytes, expression)?;
            }
            Statement::Emit { event, values } => {
                bytes.push(5);
                bytes.extend_from_slice(&event.to_be_bytes());
                push_u16(bytes, values.len())?;
                for value in values {
                    encode_expression(bytes, value)?;
                }
            }
            Statement::Transfer { recipient, amount } => {
                bytes.push(6);
                encode_expression(bytes, recipient)?;
                encode_expression(bytes, amount)?;
            }
            Statement::Call {
                target,
                selector,
                value,
            } => {
                bytes.push(7);
                encode_expression(bytes, target)?;
                encode_expression(bytes, selector)?;
                encode_expression(bytes, value)?;
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
            } => {
                bytes.push(3);
                encode_expression(bytes, condition)?;
                encode_statements(bytes, then_branch)?;
                encode_statements(bytes, else_branch)?;
            }
        }
    }
    Ok(())
}

fn encode_instruction(bytes: &mut Vec<u8>, instruction: &Instruction) {
    match instruction {
        Instruction::Constant(value_type, word) => {
            bytes.push(1);
            bytes.push(*value_type as u8);
            bytes.extend_from_slice(word);
        }
        Instruction::Parameter(index) => {
            bytes.push(2);
            bytes.extend_from_slice(&index.to_be_bytes());
        }
        Instruction::Local(index) => {
            bytes.push(9);
            bytes.extend_from_slice(&index.to_be_bytes());
        }
        Instruction::Storage(index) => {
            bytes.push(10);
            bytes.extend_from_slice(&index.to_be_bytes());
        }
        Instruction::MapStorage(index) => {
            bytes.push(16);
            bytes.extend_from_slice(&index.to_be_bytes());
        }
        Instruction::MessageSender => bytes.push(11),
        Instruction::MessageValue => bytes.push(12),
        Instruction::BlockHeight => bytes.push(13),
        Instruction::BlockTimestamp => bytes.push(14),
        Instruction::ChainId => bytes.push(15),
        Instruction::AddU64 => bytes.push(3),
        Instruction::SubU64 => bytes.push(4),
        Instruction::MulU64 => bytes.push(5),
        Instruction::DivU64 => bytes.push(6),
        Instruction::Eq => bytes.push(7),
        Instruction::LtU64 => bytes.push(8),
        Instruction::AddU256 => bytes.push(17),
        Instruction::SubU256 => bytes.push(18),
        Instruction::GteU256 => bytes.push(19),
    }
}

fn decode_instruction(reader: &mut Reader<'_>, version: u8) -> Result<Instruction> {
    match reader.byte()? {
        1 => {
            let value_type = ValueType::from_byte(reader.byte()?)?;
            let mut word = [0u8; 32];
            word.copy_from_slice(reader.take(32)?);
            validate_word(value_type, &word)?;
            Ok(Instruction::Constant(value_type, word))
        }
        2 => Ok(Instruction::Parameter(reader.u16()?)),
        3 => Ok(Instruction::AddU64),
        4 => Ok(Instruction::SubU64),
        5 => Ok(Instruction::MulU64),
        6 => Ok(Instruction::DivU64),
        7 => Ok(Instruction::Eq),
        8 => Ok(Instruction::LtU64),
        9 if version >= STATEMENT_VERSION => Ok(Instruction::Local(reader.u16()?)),
        10 if version >= STORAGE_VERSION => Ok(Instruction::Storage(reader.u16()?)),
        11 if version >= CONTEXT_VERSION => Ok(Instruction::MessageSender),
        12 if version >= CONTEXT_VERSION => Ok(Instruction::MessageValue),
        13 if version >= CONTEXT_VERSION => Ok(Instruction::BlockHeight),
        14 if version >= CONTEXT_VERSION => Ok(Instruction::BlockTimestamp),
        15 if version >= CONTEXT_VERSION => Ok(Instruction::ChainId),
        16 if version >= VERSION => Ok(Instruction::MapStorage(reader.u16()?)),
        17 if version >= VERSION => Ok(Instruction::AddU256),
        18 if version >= VERSION => Ok(Instruction::SubU256),
        19 if version >= VERSION => Ok(Instruction::GteU256),
        opcode => bail!("unknown LithoVM instruction opcode {opcode}"),
    }
}

fn decode_expression(reader: &mut Reader<'_>, version: u8) -> Result<Vec<Instruction>> {
    let instruction_count = reader.u16()? as usize;
    if instruction_count == 0 || instruction_count > MAX_INSTRUCTIONS {
        bail!("invalid LithoVM instruction count {instruction_count}");
    }
    let mut instructions = Vec::with_capacity(instruction_count);
    for _ in 0..instruction_count {
        instructions.push(decode_instruction(reader, version)?);
    }
    Ok(instructions)
}

fn decode_statements(reader: &mut Reader<'_>, version: u8, depth: usize) -> Result<Vec<Statement>> {
    if depth > MAX_BLOCK_DEPTH {
        bail!("LithoVM statement nesting exceeds {MAX_BLOCK_DEPTH}");
    }
    let statement_count = reader.u16()? as usize;
    if statement_count == 0 || statement_count > MAX_STATEMENTS {
        bail!("invalid LithoVM statement count {statement_count}");
    }
    let mut statements = Vec::with_capacity(statement_count);
    for _ in 0..statement_count {
        statements.push(match reader.byte()? {
            15 if version >= CREATE_VERSION => {
                let template = decode_expression(reader, version)?;
                let salt = decode_expression(reader, version)?;
                let initializer = decode_expression(reader, version)?;
                let value = decode_expression(reader, version)?;
                let count = reader.u16()? as usize;
                if count > MAX_PARAMETERS {
                    bail!("too many creation arguments");
                }
                let mut arguments = Vec::with_capacity(count);
                for _ in 0..count {
                    arguments.push((
                        ValueType::from_byte(reader.byte()?)?,
                        decode_expression(reader, version)?,
                    ));
                }
                Statement::Create {
                    template,
                    salt,
                    initializer,
                    value,
                    arguments,
                }
            }
            14 if version >= SYNC_VERSION => {
                let return_type = ValueType::from_byte(reader.byte()?)?;
                let target = decode_expression(reader, version)?;
                let selector = decode_expression(reader, version)?;
                let value = decode_expression(reader, version)?;
                let count = reader.u16()? as usize;
                if count > MAX_PARAMETERS {
                    bail!("too many invocation arguments");
                }
                let mut arguments = Vec::with_capacity(count);
                for _ in 0..count {
                    arguments.push((
                        ValueType::from_byte(reader.byte()?)?,
                        decode_expression(reader, version)?,
                    ));
                }
                Statement::Invoke {
                    return_type,
                    target,
                    selector,
                    value,
                    arguments,
                }
            }
            1 => Statement::Let {
                value_type: ValueType::from_byte(reader.byte()?)?,
                expression: decode_expression(reader, version)?,
            },
            8 if version >= MUTABLE_VERSION => Statement::LetMutable {
                value_type: ValueType::from_byte(reader.byte()?)?,
                expression: decode_expression(reader, version)?,
            },
            9 if version >= MUTABLE_VERSION => Statement::SetLocal {
                local: reader.u16()?,
                expression: decode_expression(reader, version)?,
            },
            10 if version >= REPEAT_VERSION => Statement::Repeat {
                count: decode_expression(reader, version)?,
                body: decode_statements(reader, version, depth + 1)?,
            },
            11 if version >= FAILURE_VERSION => {
                Statement::Require(decode_expression(reader, version)?)
            }
            12 if version >= FAILURE_VERSION => Statement::Revert,
            2 => Statement::Return(decode_expression(reader, version)?),
            3 => Statement::If {
                condition: decode_expression(reader, version)?,
                then_branch: decode_statements(reader, version, depth + 1)?,
                else_branch: decode_statements(reader, version, depth + 1)?,
            },
            4 if version >= STORAGE_VERSION => Statement::Store {
                field: reader.u16()?,
                expression: decode_expression(reader, version)?,
            },
            13 if version >= VERSION => {
                let map = reader.u16()?;
                let key_count = reader.u16()? as usize;
                if key_count == 0 || key_count > MAX_PARAMETERS {
                    bail!("invalid map key count {key_count}");
                }
                let mut keys = Vec::with_capacity(key_count);
                for _ in 0..key_count {
                    keys.push(decode_expression(reader, version)?);
                }
                Statement::MapStore {
                    map,
                    keys,
                    expression: decode_expression(reader, version)?,
                }
            }
            5 if version >= EVENT_VERSION => {
                let event = reader.u16()?;
                let count = reader.u16()? as usize;
                if count > MAX_EVENT_FIELDS {
                    bail!("event emission exceeds {MAX_EVENT_FIELDS} values");
                }
                let mut values = Vec::with_capacity(count);
                for _ in 0..count {
                    values.push(decode_expression(reader, version)?);
                }
                Statement::Emit { event, values }
            }
            6 if version >= TRANSFER_VERSION => Statement::Transfer {
                recipient: decode_expression(reader, version)?,
                amount: decode_expression(reader, version)?,
            },
            7 if version >= CALL_VERSION => Statement::Call {
                target: decode_expression(reader, version)?,
                selector: decode_expression(reader, version)?,
                value: decode_expression(reader, version)?,
            },
            opcode => bail!("unknown LithoVM statement opcode {opcode}"),
        });
    }
    Ok(statements)
}

fn validate_expression(
    instructions: &[Instruction],
    parameters: &[ValueType],
    locals: &[ValueType],
    schema: &ValidationSchema<'_>,
    return_type: ValueType,
) -> Result<()> {
    if instructions.is_empty() || instructions.len() > MAX_INSTRUCTIONS {
        bail!("expression instruction count is outside the supported range");
    }
    let mut stack = Vec::new();
    for instruction in instructions {
        match instruction {
            Instruction::Constant(value_type, word) => {
                validate_word(*value_type, word)?;
                stack.push(*value_type);
            }
            Instruction::Parameter(index) => stack.push(
                *parameters
                    .get(*index as usize)
                    .ok_or_else(|| anyhow!("expression parameter index {index} is out of range"))?,
            ),
            Instruction::Local(index) => stack.push(
                *locals
                    .get(*index as usize)
                    .ok_or_else(|| anyhow!("expression local index {index} is out of range"))?,
            ),
            Instruction::Storage(index) => stack.push(
                schema
                    .storage
                    .get(*index as usize)
                    .ok_or_else(|| anyhow!("expression storage index {index} is out of range"))?
                    .value_type,
            ),
            Instruction::MapStorage(index) => {
                let map = schema
                    .maps
                    .get(*index as usize)
                    .ok_or_else(|| anyhow!("expression map index {index} is out of range"))?;
                for key_type in map.key_types.iter().rev() {
                    pop_expected(&mut stack, *key_type)?;
                }
                stack.push(map.value_type);
            }
            Instruction::MessageSender => stack.push(ValueType::Address),
            Instruction::MessageValue => stack.push(ValueType::U256),
            Instruction::BlockHeight | Instruction::BlockTimestamp | Instruction::ChainId => {
                stack.push(ValueType::U64)
            }
            Instruction::AddU64
            | Instruction::SubU64
            | Instruction::MulU64
            | Instruction::DivU64 => {
                pop_expected(&mut stack, ValueType::U64)?;
                pop_expected(&mut stack, ValueType::U64)?;
                stack.push(ValueType::U64);
            }
            Instruction::Eq => {
                let right = stack
                    .pop()
                    .ok_or_else(|| anyhow!("expression stack underflow"))?;
                let left = stack
                    .pop()
                    .ok_or_else(|| anyhow!("expression stack underflow"))?;
                if left != right {
                    bail!("equality operands have different types");
                }
                stack.push(ValueType::Bool);
            }
            Instruction::LtU64 => {
                pop_expected(&mut stack, ValueType::U64)?;
                pop_expected(&mut stack, ValueType::U64)?;
                stack.push(ValueType::Bool);
            }
            Instruction::AddU256 | Instruction::SubU256 => {
                pop_expected(&mut stack, ValueType::U256)?;
                pop_expected(&mut stack, ValueType::U256)?;
                stack.push(ValueType::U256);
            }
            Instruction::GteU256 => {
                pop_expected(&mut stack, ValueType::U256)?;
                pop_expected(&mut stack, ValueType::U256)?;
                stack.push(ValueType::Bool);
            }
        }
    }
    if stack.as_slice() != [return_type] {
        bail!("expression must leave exactly one value matching the function return type");
    }
    Ok(())
}

fn validate_statements(
    statements: &[Statement],
    parameters: &[ValueType],
    return_type: ValueType,
    inherited_locals: &[(ValueType, bool)],
    schema: &ValidationSchema<'_>,
    depth: usize,
) -> Result<bool> {
    if depth > MAX_BLOCK_DEPTH {
        bail!("LithoVM statement nesting exceeds {MAX_BLOCK_DEPTH}");
    }
    if statements.is_empty() || statements.len() > MAX_STATEMENTS {
        bail!("statement block size is outside the supported range");
    }
    let (mut locals, mut mutable): (Vec<_>, Vec<_>) = inherited_locals.iter().copied().unzip();
    let mut terminal = false;
    for statement in statements {
        if terminal {
            bail!("unreachable statement after terminal return or branch");
        }
        match statement {
            Statement::Create {
                template,
                salt,
                initializer,
                value,
                arguments,
            } => {
                for (expression, ty) in [
                    (template, ValueType::Address),
                    (salt, ValueType::Bytes32),
                    (initializer, ValueType::Bytes32),
                    (value, ValueType::U256),
                ] {
                    validate_expression(expression, parameters, &locals, schema, ty)?;
                }
                if arguments.len() > MAX_PARAMETERS {
                    bail!("too many creation arguments");
                }
                for (ty, expression) in arguments {
                    validate_expression(expression, parameters, &locals, schema, *ty)?;
                }
                if locals.len() >= MAX_LOCALS {
                    bail!("too many local bindings");
                }
                locals.push(ValueType::Address);
                mutable.push(false);
            }
            Statement::Invoke {
                return_type,
                target,
                selector,
                value,
                arguments,
            } => {
                validate_expression(target, parameters, &locals, schema, ValueType::Address)?;
                validate_expression(selector, parameters, &locals, schema, ValueType::Bytes32)?;
                validate_expression(value, parameters, &locals, schema, ValueType::U256)?;
                if arguments.len() > MAX_PARAMETERS {
                    bail!("too many invocation arguments");
                }
                for (ty, expression) in arguments {
                    validate_expression(expression, parameters, &locals, schema, *ty)?;
                }
                if locals.len() >= MAX_LOCALS {
                    bail!("too many local bindings");
                }
                locals.push(*return_type);
                mutable.push(false);
            }
            Statement::Let {
                value_type,
                expression,
            } => {
                validate_expression(expression, parameters, &locals, schema, *value_type)?;
                if locals.len() >= MAX_LOCALS {
                    bail!("function exceeds {MAX_LOCALS} local bindings");
                }
                locals.push(*value_type);
                mutable.push(false);
            }
            Statement::LetMutable {
                value_type,
                expression,
            } => {
                validate_expression(expression, parameters, &locals, schema, *value_type)?;
                if locals.len() >= MAX_LOCALS {
                    bail!("function exceeds {MAX_LOCALS} local bindings");
                }
                locals.push(*value_type);
                mutable.push(true);
            }
            Statement::SetLocal { local, expression } => {
                let index = *local as usize;
                let value_type = *locals
                    .get(index)
                    .ok_or_else(|| anyhow!("local assignment index {local} is out of range"))?;
                if !mutable[index] {
                    bail!("local assignment targets an immutable binding");
                }
                validate_expression(expression, parameters, &locals, schema, value_type)?;
            }
            Statement::Repeat { count, body } => {
                validate_expression(count, parameters, &locals, schema, ValueType::U64)?;
                let bindings = locals
                    .iter()
                    .copied()
                    .zip(mutable.iter().copied())
                    .collect::<Vec<_>>();
                validate_statements(body, parameters, return_type, &bindings, schema, depth + 1)?;
            }
            Statement::Require(condition) => {
                validate_expression(condition, parameters, &locals, schema, ValueType::Bool)?;
            }
            Statement::Revert => terminal = true,
            Statement::Return(expression) => {
                validate_expression(expression, parameters, &locals, schema, return_type)?;
                terminal = true;
            }
            Statement::Store { field, expression } => {
                let value_type = schema
                    .storage
                    .get(*field as usize)
                    .ok_or_else(|| anyhow!("store field index {field} is out of range"))?
                    .value_type;
                validate_expression(expression, parameters, &locals, schema, value_type)?;
            }
            Statement::MapStore {
                map,
                keys,
                expression,
            } => {
                let definition = schema
                    .maps
                    .get(*map as usize)
                    .ok_or_else(|| anyhow!("map store index {map} is out of range"))?;
                if keys.len() != definition.key_types.len() {
                    bail!("map store key count does not match schema");
                }
                for (key, key_type) in keys.iter().zip(&definition.key_types) {
                    validate_expression(key, parameters, &locals, schema, *key_type)?;
                }
                validate_expression(
                    expression,
                    parameters,
                    &locals,
                    schema,
                    definition.value_type,
                )?;
            }
            Statement::Emit { event, values } => {
                let definition = schema
                    .events
                    .get(*event as usize)
                    .ok_or_else(|| anyhow!("event index {event} is out of range"))?;
                if values.len() != definition.fields.len() {
                    bail!(
                        "event '{}' value count does not match schema",
                        definition.name
                    );
                }
                for (value, field) in values.iter().zip(&definition.fields) {
                    validate_expression(value, parameters, &locals, schema, field.value_type)?;
                }
            }
            Statement::Transfer { recipient, amount } => {
                validate_expression(recipient, parameters, &locals, schema, ValueType::Address)?;
                validate_expression(amount, parameters, &locals, schema, ValueType::U256)?;
            }
            Statement::Call {
                target,
                selector,
                value,
            } => {
                validate_expression(target, parameters, &locals, schema, ValueType::Address)?;
                validate_expression(selector, parameters, &locals, schema, ValueType::Bytes32)?;
                validate_expression(value, parameters, &locals, schema, ValueType::U256)?;
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
            } => {
                validate_expression(condition, parameters, &locals, schema, ValueType::Bool)?;
                let bindings = locals
                    .iter()
                    .copied()
                    .zip(mutable.iter().copied())
                    .collect::<Vec<_>>();
                let then_terminal = validate_statements(
                    then_branch,
                    parameters,
                    return_type,
                    &bindings,
                    schema,
                    depth + 1,
                )?;
                let else_terminal = validate_statements(
                    else_branch,
                    parameters,
                    return_type,
                    &bindings,
                    schema,
                    depth + 1,
                )?;
                terminal = then_terminal && else_terminal;
            }
        }
    }
    Ok(terminal)
}

fn pop_expected(stack: &mut Vec<ValueType>, expected: ValueType) -> Result<()> {
    let actual = stack
        .pop()
        .ok_or_else(|| anyhow!("expression stack underflow"))?;
    if actual != expected {
        bail!(
            "expression expected {}, found {}",
            expected.name(),
            actual.name()
        );
    }
    Ok(())
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn validate_name(value: &str, kind: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_NAME_BYTES
        || !value.is_ascii()
        || !is_identifier(value)
    {
        bail!("invalid LithoVM {kind} name '{value}'");
    }
    Ok(())
}

fn push_name(bytes: &mut Vec<u8>, value: &str) -> Result<()> {
    push_u16(bytes, value.len())?;
    bytes.extend_from_slice(value.as_bytes());
    Ok(())
}

fn read_name(reader: &mut Reader<'_>, kind: &str) -> Result<String> {
    let length = reader.u16()? as usize;
    if length == 0 || length > MAX_NAME_BYTES {
        bail!("invalid LithoVM {kind} name length {length}");
    }
    let value = std::str::from_utf8(reader.take(length)?)
        .map_err(|_| anyhow!("{kind} name is not valid UTF-8"))?
        .to_owned();
    validate_name(&value, kind)?;
    Ok(value)
}

fn push_u16(bytes: &mut Vec<u8>, value: usize) -> Result<()> {
    let value = u16::try_from(value).map_err(|_| anyhow!("value exceeds u16 encoding"))?;
    bytes.extend_from_slice(&value.to_be_bytes());
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| anyhow!("bytecode offset overflow"))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| anyhow!("truncated LithoVM bytecode"))?;
        self.position = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Program {
        Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "answer".into(),
                parameters: vec![],
                return_type: ValueType::U64,
                return_value: ReturnValue::Constant({
                    let mut word = [0u8; 32];
                    word[31] = 42;
                    word
                }),
            }],
        }
    }

    fn storage_table_end(storage: &[StorageField]) -> usize {
        let mut offset = MAGIC.len() + 1 + 2;
        for field in storage {
            offset += 2 + field.name.len() + 1;
        }
        offset
    }

    fn strip_empty_map_table(mut bytes: Vec<u8>, storage: &[StorageField]) -> Vec<u8> {
        let offset = storage_table_end(storage);
        assert_eq!(&bytes[offset..offset + 2], &[0, 0]);
        bytes.drain(offset..offset + 2);
        bytes
    }

    fn strip_empty_map_and_event_tables(bytes: Vec<u8>, storage: &[StorageField]) -> Vec<u8> {
        let mut bytes = strip_empty_map_table(bytes, storage);
        let offset = storage_table_end(storage);
        assert_eq!(&bytes[offset..offset + 2], &[0, 0]);
        bytes.drain(offset..offset + 2);
        bytes
    }

    #[test]
    fn versioned_program_round_trips_deterministically() {
        let program = sample();
        let first = program.encode().unwrap();
        assert_eq!(first, program.encode().unwrap());
        assert_eq!(parse(&first).unwrap(), program);
    }

    #[test]
    fn rejects_truncation_trailing_bytes_and_unknown_versions() {
        let bytes = sample().encode().unwrap();
        for length in 0..bytes.len() {
            assert!(parse(&bytes[..length]).is_err(), "accepted length {length}");
        }
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(parse(&trailing).is_err());
        let mut version = bytes;
        version[MAGIC.len()] = CREATE_VERSION + 1;
        assert!(parse(&version).is_err());
    }

    #[test]
    fn rejects_non_canonical_constant_words() {
        let mut program = sample();
        program.functions[0].return_value = ReturnValue::Constant([0xff; 32]);
        assert!(program.encode().is_err());
    }

    #[test]
    fn typed_expression_round_trips_and_rejects_bad_stacks() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "add".into(),
                parameters: vec![ValueType::U64],
                return_type: ValueType::U64,
                return_value: ReturnValue::Expression(vec![
                    Instruction::Parameter(0),
                    Instruction::Constant(ValueType::U64, {
                        let mut word = [0; 32];
                        word[31] = 1;
                        word
                    }),
                    Instruction::AddU64,
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);
        let mut v3 = strip_empty_map_and_event_tables(bytes.clone(), &program.storage);
        v3[MAGIC.len()] = STORAGE_VERSION;
        assert_eq!(parse(&v3).unwrap(), program);

        let mut invalid = program;
        invalid.functions[0].return_value = ReturnValue::Expression(vec![Instruction::AddU64]);
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn structured_statements_round_trip_and_validate_local_types() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "choose".into(),
                parameters: vec![ValueType::U64, ValueType::Bool],
                return_type: ValueType::U64,
                return_value: ReturnValue::Statements(vec![
                    Statement::Let {
                        value_type: ValueType::U64,
                        expression: vec![Instruction::Parameter(0)],
                    },
                    Statement::If {
                        condition: vec![Instruction::Parameter(1)],
                        then_branch: vec![Statement::Return(vec![Instruction::Local(0)])],
                        else_branch: vec![Statement::Return(vec![Instruction::Constant(
                            ValueType::U64,
                            {
                                let mut word = [0; 32];
                                word[31] = 9;
                                word
                            },
                        )])],
                    },
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(bytes[MAGIC.len()], VERSION);
        assert_eq!(parse(&bytes).unwrap(), program);

        let mut invalid = program;
        let ReturnValue::Statements(statements) = &mut invalid.functions[0].return_value else {
            unreachable!()
        };
        let Statement::If { then_branch, .. } = &mut statements[1] else {
            unreachable!()
        };
        *then_branch = vec![Statement::Return(vec![Instruction::Local(1)])];
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn older_artifact_versions_remain_readable() {
        for version in [LEGACY_VERSION, STATEMENT_VERSION] {
            let mut bytes = sample().encode().unwrap();
            bytes[MAGIC.len()] = version;
            bytes.drain(MAGIC.len() + 1..MAGIC.len() + 7);
            assert_eq!(parse(&bytes).unwrap(), sample());
        }

        for version in [STORAGE_VERSION, CONTEXT_VERSION] {
            let mut bytes = strip_empty_map_and_event_tables(sample().encode().unwrap(), &[]);
            bytes[MAGIC.len()] = version;
            assert_eq!(parse(&bytes).unwrap(), sample());
        }
        let mut v5 = strip_empty_map_table(sample().encode().unwrap(), &[]);
        v5[MAGIC.len()] = EVENT_VERSION;
        assert_eq!(parse(&v5).unwrap(), sample());
        let mut v6 = strip_empty_map_table(sample().encode().unwrap(), &[]);
        v6[MAGIC.len()] = TRANSFER_VERSION;
        assert_eq!(parse(&v6).unwrap(), sample());
        let mut v7 = strip_empty_map_table(sample().encode().unwrap(), &[]);
        v7[MAGIC.len()] = CALL_VERSION;
        assert_eq!(parse(&v7).unwrap(), sample());
        let mut v8 = strip_empty_map_table(sample().encode().unwrap(), &[]);
        v8[MAGIC.len()] = MUTABLE_VERSION;
        assert_eq!(parse(&v8).unwrap(), sample());
        let mut v9 = strip_empty_map_table(sample().encode().unwrap(), &[]);
        v9[MAGIC.len()] = REPEAT_VERSION;
        assert_eq!(parse(&v9).unwrap(), sample());
        let mut v10 = strip_empty_map_table(sample().encode().unwrap(), &[]);
        v10[MAGIC.len()] = FAILURE_VERSION;
        assert_eq!(parse(&v10).unwrap(), sample());
    }

    #[test]
    fn storage_schema_reads_and_writes_round_trip() {
        let program = Program {
            storage: vec![StorageField {
                name: "count".into(),
                value_type: ValueType::U64,
            }],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "set".into(),
                parameters: vec![ValueType::U64],
                return_type: ValueType::U64,
                return_value: ReturnValue::Statements(vec![
                    Statement::Store {
                        field: 0,
                        expression: vec![Instruction::Parameter(0)],
                    },
                    Statement::Return(vec![Instruction::Storage(0)]),
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);
        let mut storage_v3 = strip_empty_map_and_event_tables(bytes.clone(), &program.storage);
        storage_v3[MAGIC.len()] = STORAGE_VERSION;
        assert_eq!(parse(&storage_v3).unwrap(), program);

        let mut invalid = program;
        invalid.functions[0].return_value = ReturnValue::Statements(vec![
            Statement::Store {
                field: 1,
                expression: vec![Instruction::Parameter(0)],
            },
            Statement::Return(vec![Instruction::Storage(0)]),
        ]);
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn typed_map_storage_and_u256_operations_round_trip() {
        let program = Program {
            storage: vec![],
            maps: vec![MapField {
                name: "balances".into(),
                key_types: vec![ValueType::Address],
                value_type: ValueType::U256,
            }],
            events: vec![],
            functions: vec![Function {
                name: "credit".into(),
                parameters: vec![ValueType::Address, ValueType::U256],
                return_type: ValueType::U256,
                return_value: ReturnValue::Statements(vec![
                    Statement::MapStore {
                        map: 0,
                        keys: vec![vec![Instruction::Parameter(0)]],
                        expression: vec![
                            Instruction::Parameter(0),
                            Instruction::MapStorage(0),
                            Instruction::Parameter(1),
                            Instruction::AddU256,
                        ],
                    },
                    Statement::Return(vec![Instruction::Parameter(0), Instruction::MapStorage(0)]),
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);

        let mut invalid = program;
        invalid.maps[0].key_types[0] = ValueType::Bool;
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn execution_context_instructions_round_trip_with_static_types() {
        for (instruction, return_type) in [
            (Instruction::MessageSender, ValueType::Address),
            (Instruction::MessageValue, ValueType::U256),
            (Instruction::BlockHeight, ValueType::U64),
            (Instruction::BlockTimestamp, ValueType::U64),
            (Instruction::ChainId, ValueType::U64),
        ] {
            let program = Program {
                storage: vec![],
                maps: vec![],
                events: vec![],
                functions: vec![Function {
                    name: "context".into(),
                    parameters: vec![],
                    return_type,
                    return_value: ReturnValue::Expression(vec![instruction]),
                }],
            };
            let bytes = program.encode().unwrap();
            assert_eq!(parse(&bytes).unwrap(), program);
            let mut mislabeled_v3 = strip_empty_map_and_event_tables(bytes, &[]);
            mislabeled_v3[MAGIC.len()] = STORAGE_VERSION;
            assert!(parse(&mislabeled_v3).is_err());
        }
    }

    #[test]
    fn typed_event_schema_and_emission_round_trip() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![EventDefinition {
                name: "Changed".into(),
                fields: vec![StorageField {
                    name: "value".into(),
                    value_type: ValueType::U64,
                }],
            }],
            functions: vec![Function {
                name: "change".into(),
                parameters: vec![ValueType::U64],
                return_type: ValueType::U64,
                return_value: ReturnValue::Statements(vec![
                    Statement::Emit {
                        event: 0,
                        values: vec![vec![Instruction::Parameter(0)]],
                    },
                    Statement::Return(vec![Instruction::Parameter(0)]),
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);

        let mut invalid = program;
        invalid.functions[0].return_value = ReturnValue::Statements(vec![
            Statement::Emit {
                event: 1,
                values: vec![vec![Instruction::Parameter(0)]],
            },
            Statement::Return(vec![Instruction::Parameter(0)]),
        ]);
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn native_transfer_statement_round_trips_with_static_types() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "pay".into(),
                parameters: vec![ValueType::Address, ValueType::U256],
                return_type: ValueType::U256,
                return_value: ReturnValue::Statements(vec![
                    Statement::Transfer {
                        recipient: vec![Instruction::Parameter(0)],
                        amount: vec![Instruction::Parameter(1)],
                    },
                    Statement::Return(vec![Instruction::Parameter(1)]),
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);
        let mut mislabeled_v5 = bytes;
        mislabeled_v5[MAGIC.len()] = EVENT_VERSION;
        assert!(parse(&mislabeled_v5).is_err());
    }

    #[test]
    fn contract_call_statement_round_trips_with_static_types() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "invoke".into(),
                parameters: vec![ValueType::Address, ValueType::Bytes32, ValueType::U256],
                return_type: ValueType::U256,
                return_value: ReturnValue::Statements(vec![
                    Statement::Call {
                        target: vec![Instruction::Parameter(0)],
                        selector: vec![Instruction::Parameter(1)],
                        value: vec![Instruction::Parameter(2)],
                    },
                    Statement::Return(vec![Instruction::Parameter(2)]),
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);
        let mut mislabeled_v6 = bytes;
        mislabeled_v6[MAGIC.len()] = TRANSFER_VERSION;
        assert!(parse(&mislabeled_v6).is_err());
    }

    #[test]
    fn mutable_local_statements_round_trip_and_reject_immutable_assignment() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "increment".into(),
                parameters: vec![ValueType::U64],
                return_type: ValueType::U64,
                return_value: ReturnValue::Statements(vec![
                    Statement::LetMutable {
                        value_type: ValueType::U64,
                        expression: vec![Instruction::Parameter(0)],
                    },
                    Statement::SetLocal {
                        local: 0,
                        expression: vec![
                            Instruction::Local(0),
                            Instruction::Constant(ValueType::U64, {
                                let mut word = [0; 32];
                                word[31] = 1;
                                word
                            }),
                            Instruction::AddU64,
                        ],
                    },
                    Statement::Return(vec![Instruction::Local(0)]),
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);
        let mut mislabeled_v7 = bytes;
        mislabeled_v7[MAGIC.len()] = CALL_VERSION;
        assert!(parse(&mislabeled_v7).is_err());

        let mut invalid = program;
        let ReturnValue::Statements(statements) = &mut invalid.functions[0].return_value else {
            unreachable!()
        };
        statements[0] = Statement::Let {
            value_type: ValueType::U64,
            expression: vec![Instruction::Parameter(0)],
        };
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn repeat_statement_round_trips_and_requires_u64_count() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "count".into(),
                parameters: vec![ValueType::U64],
                return_type: ValueType::U64,
                return_value: ReturnValue::Statements(vec![
                    Statement::LetMutable {
                        value_type: ValueType::U64,
                        expression: vec![Instruction::Constant(ValueType::U64, [0; 32])],
                    },
                    Statement::Repeat {
                        count: vec![Instruction::Parameter(0)],
                        body: vec![Statement::SetLocal {
                            local: 0,
                            expression: vec![
                                Instruction::Local(0),
                                Instruction::Constant(ValueType::U64, {
                                    let mut word = [0; 32];
                                    word[31] = 1;
                                    word
                                }),
                                Instruction::AddU64,
                            ],
                        }],
                    },
                    Statement::Return(vec![Instruction::Local(0)]),
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);
        let mut mislabeled_v8 = bytes;
        mislabeled_v8[MAGIC.len()] = MUTABLE_VERSION;
        assert!(parse(&mislabeled_v8).is_err());

        let mut invalid = program;
        let ReturnValue::Statements(statements) = &mut invalid.functions[0].return_value else {
            unreachable!()
        };
        let Statement::Repeat { count, .. } = &mut statements[1] else {
            unreachable!()
        };
        *count = vec![Instruction::Constant(ValueType::Bool, [0; 32])];
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn canonical_selector_matches_the_published_vector() {
        let function = Function {
            name: "transfer".into(),
            parameters: vec![ValueType::Address, ValueType::U256],
            return_type: ValueType::Bool,
            return_value: ReturnValue::Constant([0; 32]),
        };

        assert_eq!(function_signature(&function), "transfer(address,u256)");
        assert_eq!(
            hex::encode(function_selector(&function)),
            "f61367304e4e32065cad538b10a44bb599f78e0771530108572d225c74122c1f"
        );
    }

    #[test]
    fn failure_statements_round_trip_and_require_bool() {
        let program = Program {
            storage: vec![],
            maps: vec![],
            events: vec![],
            functions: vec![Function {
                name: "guarded".into(),
                parameters: vec![ValueType::Bool],
                return_type: ValueType::U64,
                return_value: ReturnValue::Statements(vec![
                    Statement::Require(vec![Instruction::Parameter(0)]),
                    Statement::If {
                        condition: vec![Instruction::Parameter(0)],
                        then_branch: vec![Statement::Return(vec![Instruction::Constant(
                            ValueType::U64,
                            [0; 32],
                        )])],
                        else_branch: vec![Statement::Revert],
                    },
                ]),
            }],
        };
        let bytes = program.encode().unwrap();
        assert_eq!(parse(&bytes).unwrap(), program);

        let mut mislabeled_v9 = bytes;
        mislabeled_v9[MAGIC.len()] = REPEAT_VERSION;
        assert!(parse(&mislabeled_v9).is_err());

        let mut invalid = program;
        let ReturnValue::Statements(statements) = &mut invalid.functions[0].return_value else {
            unreachable!()
        };
        statements[0] = Statement::Require(vec![Instruction::Constant(ValueType::U64, [0; 32])]);
        assert!(invalid.encode().is_err());
    }
}
