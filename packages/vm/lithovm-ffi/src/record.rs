//! New v17 contracts use an explicitly versioned binary record, avoiding hex
//! expansion in billable state. Existing v2 JSON records retain their exact codec.
use super::*;
use lithovm_bytecode::{COMPACT_CONSTANT_VERSION, MAGIC};

const BINARY_MAGIC: &[u8; 5] = b"LCTR\x03";

pub(super) struct DecodedRecord {
    pub bytecode: Vec<u8>,
    pub code_hash: [u8; 32],
    pub storage: Vec<u8>,
}

pub(super) fn encode_record(contract: &DeployedContract) -> Result<Vec<u8>, String> {
    let storage = contract
        .storage
        .to_persisted_scalar_bytes()
        .map_err(|e| e.to_string())?;
    if contract.bytecode.get(MAGIC.len()) != Some(&COMPACT_CONSTANT_VERSION) {
        return serde_json::to_vec(&Stored {
            version: 2,
            bytecode: hex(&contract.bytecode),
            code_hash: hex(&contract.code_hash),
            storage: hex(&storage),
        })
        .map_err(|e| e.to_string());
    }
    if contract.bytecode.len() > MAX_CHILD_CODE_BYTES || storage.len() > MAX_BYTES {
        return Err("binary contract record exceeds limit".into());
    }
    let mut bytes = BINARY_MAGIC.to_vec();
    bytes.extend_from_slice(&(contract.bytecode.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&contract.bytecode);
    bytes.extend_from_slice(&contract.code_hash);
    bytes.extend_from_slice(&(storage.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&storage);
    Ok(bytes)
}

pub(super) fn decode_record(bytes: &[u8]) -> Result<DecodedRecord, String> {
    if bytes.len() > MAX_BYTES {
        return Err("contract record exceeds limit".into());
    }
    if !bytes.starts_with(BINARY_MAGIC) {
        let record: Stored = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if record.version != 2 {
            return Err("unsupported contract record; explicit migration required".into());
        }
        let bytecode = decode(&record.bytecode)?;
        if bytecode
            .get(MAGIC.len())
            .is_some_and(|v| *v >= COMPACT_CONSTANT_VERSION)
        {
            return Err("v17 contract requires binary record".into());
        }
        let code_hash: [u8; 32] = decode(&record.code_hash)?
            .try_into()
            .map_err(|_| "invalid stored hash length")?;
        // Retain the old v2 hash-string canonicality check (including case).
        if hex(&code_hash) != record.code_hash {
            return Err("stored code hash mismatch".into());
        }
        return Ok(DecodedRecord {
            bytecode,
            code_hash,
            storage: decode(&record.storage)?,
        });
    }
    let mut remaining = &bytes[BINARY_MAGIC.len()..];
    let code_len = length(&mut remaining, MAX_CHILD_CODE_BYTES)?;
    let bytecode = take(&mut remaining, code_len)?.to_vec();
    if !bytecode.starts_with(MAGIC) || bytecode.get(MAGIC.len()) != Some(&COMPACT_CONSTANT_VERSION)
    {
        return Err("binary record requires v17 bytecode".into());
    }
    let code_hash = take(&mut remaining, 32)?.try_into().unwrap();
    let storage_len = length(&mut remaining, MAX_BYTES)?;
    let storage = take(&mut remaining, storage_len)?.to_vec();
    if !remaining.is_empty() {
        return Err("trailing binary contract record data".into());
    }
    Ok(DecodedRecord {
        bytecode,
        code_hash,
        storage,
    })
}

fn length(bytes: &mut &[u8], limit: usize) -> Result<usize, String> {
    let len = u32::from_be_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
    if len > limit {
        return Err("binary contract field exceeds limit".into());
    }
    Ok(len)
}

fn take<'a>(bytes: &mut &'a [u8], len: usize) -> Result<&'a [u8], String> {
    if bytes.len() < len {
        return Err("truncated binary contract record".into());
    }
    let (value, rest) = bytes.split_at(len);
    *bytes = rest;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn contract(compact: bool) -> DeployedContract {
        let source = if compact {
            "contract C { pub fn value(data: bytes) -> u64 { return bytes_get(data, 0) + 1; } }"
        } else {
            "contract C { pub fn value() -> u64 { return 42; } }"
        };
        let code = decode(&lithic_lithovm::compile(source).unwrap().bytecode).unwrap();
        DeployedContract {
            code_hash: code_hash(&code),
            bytecode: code,
            storage: Storage::default(),
            entrypoints: BTreeMap::new(),
        }
    }

    #[test]
    fn binary_record_round_trip_and_legacy_json_bytes_are_stable() {
        for compact in [false, true] {
            let contract = contract(compact);
            let encoded = encode_record(&contract).unwrap();
            assert_eq!(encoded.starts_with(BINARY_MAGIC), compact);
            if compact {
                assert_eq!(
                    hex::encode(&encoded),
                    include_str!(
                        "../../../../integration/native-chain/testdata/native-v17-record.hex"
                    )
                    .trim()
                );
            }
            let decoded = decode_record(&encoded).unwrap();
            assert_eq!(decoded.bytecode, contract.bytecode);
            assert_eq!(decoded.code_hash, contract.code_hash);
            assert_eq!(
                decoded.storage,
                contract.storage.to_persisted_scalar_bytes().unwrap()
            );
            if !compact {
                let mut noncanonical: Stored = serde_json::from_slice(&encoded).unwrap();
                noncanonical.code_hash =
                    format!("0x{}", hex::encode(contract.code_hash).to_uppercase());
                assert!(decode_record(&serde_json::to_vec(&noncanonical).unwrap()).is_err());
                assert_eq!(
                    encoded,
                    serde_json::to_vec(&Stored {
                        version: 2,
                        bytecode: hex(&contract.bytecode),
                        code_hash: hex(&contract.code_hash),
                        storage: hex(&decoded.storage)
                    })
                    .unwrap()
                );
            }
            let mut state = super::super::tests::state();
            let mut tx = state.begin_transaction().unwrap();
            tx.writes.insert(key(&[1; 32]), encoded.clone());
            let loaded = tx.load_contract(&[1; 32]).unwrap().unwrap();
            tx.store_contract([1; 32], loaded).unwrap();
            assert_eq!(tx.writes[&key(&[1; 32])], encoded);
        }
    }

    #[test]
    fn binary_record_rejects_truncation_trailing_data_oversize_and_codec_confusion() {
        let contract = contract(true);
        let encoded = encode_record(&contract).unwrap();
        for end in 0..encoded.len() {
            assert!(
                decode_record(&encoded[..end]).is_err(),
                "accepted truncation at {end}"
            );
        }
        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(decode_record(&trailing).is_err());
        for offset in [
            BINARY_MAGIC.len(),
            BINARY_MAGIC.len() + 4 + contract.bytecode.len() + 32,
        ] {
            let mut oversized = encoded.clone();
            oversized[offset..offset + 4].copy_from_slice(&u32::MAX.to_be_bytes());
            assert!(decode_record(&oversized).is_err());
        }
        let mut downgraded = encoded.clone();
        downgraded[BINARY_MAGIC.len() + 4 + MAGIC.len()] = 16;
        assert!(decode_record(&downgraded).is_err());
        let wrong_codec = serde_json::to_vec(&Stored {
            version: 2,
            bytecode: hex(&contract.bytecode),
            code_hash: hex(&contract.code_hash),
            storage: hex(&contract.storage.to_persisted_scalar_bytes().unwrap()),
        })
        .unwrap();
        assert!(decode_record(&wrong_codec).is_err());
        for version in [0, 1, 2, 4, 255] {
            let mut unknown = encoded.clone();
            unknown[4] = version;
            assert!(decode_record(&unknown).is_err());
        }
    }

    #[test]
    fn binary_record_rejects_corrupt_code_hash_before_execution() {
        let contract = contract(true);
        let mut encoded = encode_record(&contract).unwrap();
        encoded[BINARY_MAGIC.len() + 4 + contract.bytecode.len()] ^= 1;
        let mut state = super::super::tests::state();
        let mut tx = state.begin_transaction().unwrap();
        tx.writes.insert(key(&[1; 32]), encoded);
        assert_eq!(
            tx.load_contract(&[1; 32]).unwrap_err(),
            "stored code hash mismatch"
        );
    }
}
