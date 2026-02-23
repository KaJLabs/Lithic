use anyhow::{anyhow, Result};

pub const MAGIC: &[u8; 7] = b"LITHOVM";

#[derive(Debug, Clone)]
pub struct Bytecode {
    pub payload: Vec<u8>,
}

pub fn parse(bytes: &[u8]) -> Result<Bytecode> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(anyhow!("invalid bytecode magic"));
    }
    Ok(Bytecode { payload: bytes.to_vec() })
}
