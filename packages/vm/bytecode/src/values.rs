//! Candidate dynamic call-value envelope for v12 VM value adapters, not v11 calls.
//! Fixed words retain their canonical validation; strings preserve exact UTF-8.
use crate::{validate_word, ValueType};
use anyhow::{bail, Result};

const MAGIC: &[u8; 5] = b"LVAL\x01";
pub const MAX_VALUES: usize = 64;
pub const MAX_STRING_BYTES: usize = 4096;
pub const MAX_ENVELOPE_BYTES: usize = 65536;
const STRING_TAG: u8 = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Word(ValueType, [u8; 32]),
    String(String),
}

/// Validates all bounds before allocating an encoded payload.
pub fn encode(values: &[Value]) -> Result<Vec<u8>> {
    let size = encoded_size(values)?;
    encode_validated(values, size)
}

/// Validate without allocating an encoded copy.
pub fn encoded_size(values: &[Value]) -> Result<usize> {
    if values.len() > MAX_VALUES {
        bail!("too many ABI values");
    }
    let mut size = MAGIC.len() + 2;
    for value in values {
        size += match value {
            Value::Word(ty, word) => {
                validate_word(*ty, word)?;
                33
            }
            Value::String(text) => {
                if text.len() > MAX_STRING_BYTES {
                    bail!("string exceeds ABI byte limit");
                }
                3 + text.len()
            }
        };
        if size > MAX_ENVELOPE_BYTES {
            bail!("ABI envelope exceeds byte limit");
        }
    }
    Ok(size)
}

fn encode_validated(values: &[Value], size: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(size);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(values.len() as u16).to_be_bytes());
    for value in values {
        match value {
            Value::Word(ty, word) => {
                bytes.push(*ty as u8);
                bytes.extend_from_slice(word);
            }
            Value::String(text) => {
                bytes.push(STRING_TAG);
                bytes.extend_from_slice(&(text.len() as u16).to_be_bytes());
                bytes.extend_from_slice(text.as_bytes());
            }
        }
    }
    Ok(bytes)
}

pub fn decode(bytes: &[u8]) -> Result<Vec<Value>> {
    if bytes.len() > MAX_ENVELOPE_BYTES {
        bail!("ABI envelope exceeds byte limit");
    }
    let mut rest = bytes;
    if take(&mut rest, MAGIC.len())? != MAGIC {
        bail!("unsupported ABI envelope");
    }
    let count = read_u16(&mut rest)?;
    if count > MAX_VALUES {
        bail!("too many ABI values");
    }
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let tag = take(&mut rest, 1)?[0];
        if tag == STRING_TAG {
            let len = read_u16(&mut rest)?;
            if len > MAX_STRING_BYTES {
                bail!("string exceeds ABI byte limit");
            }
            let text = std::str::from_utf8(take(&mut rest, len)?)?;
            values.push(Value::String(text.to_owned()));
        } else {
            let ty = ValueType::from_byte(tag)?;
            let mut word = [0; 32];
            word.copy_from_slice(take(&mut rest, 32)?);
            validate_word(ty, &word)?;
            values.push(Value::Word(ty, word));
        }
    }
    if !rest.is_empty() {
        bail!("trailing ABI bytes");
    }
    Ok(values)
}

fn take<'a>(rest: &mut &'a [u8], len: usize) -> Result<&'a [u8]> {
    if rest.len() < len {
        bail!("truncated ABI envelope");
    }
    let (value, tail) = rest.split_at(len);
    *rest = tail;
    Ok(value)
}

fn read_u16(rest: &mut &[u8]) -> Result<usize> {
    let bytes = take(rest, 2)?;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
}
