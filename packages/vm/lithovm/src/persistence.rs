use super::*;
use serde::{Deserialize, Serialize};

const MAX_STATE_BYTES: usize = 1024 * 1024;
type MapEntry = (String, Vec<[u8; 32]>, [u8; 32]);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    version: u8,
    words: BTreeMap<String, [u8; 32]>,
    strings: BTreeMap<String, String>,
    maps: Vec<MapEntry>,
}

impl Storage {
    /// Bounded canonical persistence candidate. Not an approved consensus layout.
    pub fn to_persisted_bytes(&self) -> Result<Vec<u8>> {
        let snapshot = Snapshot {
            version: 1,
            words: self.values.clone(),
            strings: self
                .strings
                .iter()
                .map(|(k, v)| (k.clone(), v.to_string()))
                .collect(),
            maps: self
                .maps
                .iter()
                .flat_map(|(name, entries)| {
                    entries
                        .iter()
                        .map(|(keys, value)| (name.clone(), keys.clone(), *value))
                })
                .collect(),
        };
        let bytes = serde_json::to_vec(&snapshot)?;
        if bytes.len() > MAX_STATE_BYTES {
            bail!("persisted state exceeds byte limit");
        }
        Ok(bytes)
    }

    pub fn from_persisted_bytes(bytes: &[u8], program: &Program) -> Result<Self> {
        if bytes.len() > MAX_STATE_BYTES {
            bail!("persisted state exceeds byte limit");
        }
        let snapshot: Snapshot = serde_json::from_slice(bytes)?;
        if snapshot.version != 1 {
            bail!("unsupported storage encoding");
        }
        let mut storage = Self {
            values: snapshot.words,
            strings: snapshot
                .strings
                .into_iter()
                .map(|(k, v)| (k, Arc::from(v)))
                .collect(),
            maps: BTreeMap::new(),
            map_reader: None,
        };
        for (name, keys, value) in snapshot.maps {
            if storage
                .maps
                .entry(name)
                .or_default()
                .insert(keys, value)
                .is_some()
            {
                bail!("duplicate map entry");
            }
        }
        if storage.to_persisted_bytes()? != bytes {
            bail!("noncanonical storage encoding");
        }
        prepare_storage(program, &mut storage)?;
        Ok(storage)
    }
}

impl Storage {
    /// V2 contract metadata excludes map values; individual map keys are
    /// persisted by the chain adapter and loaded only when accessed.
    pub fn to_persisted_scalar_bytes(&self) -> Result<Vec<u8>> {
        let mut scalars = self.clone();
        scalars.maps.clear();
        scalars.map_reader = None;
        scalars.to_persisted_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Program {
        let artifact = lithic_lithovm::compile("contract C { state { name: string; owner: address; balances: map<address, u256>; allowances: map<address, map<address, u256>>; } pub fn read() -> string { return self.name; } }").unwrap();
        parse(&hex::decode(&artifact.bytecode[2..]).unwrap()).unwrap()
    }
    fn word(n: u8) -> [u8; 32] {
        let mut w = [0; 32];
        w[31] = n;
        w
    }

    #[test]
    fn canonical_snapshot_preserves_unicode_maps_and_defaults() {
        let program = fixture();
        let mut storage = Storage::default();
        storage
            .strings
            .insert("name".into(), Arc::from("LAX \u{1faa8}\0"));
        storage.values.insert("owner".into(), word(9));
        storage.set_map_word("balances", vec![word(9)], word(42));
        storage.set_map_word("allowances", vec![word(9), word(10)], word(7));
        let bytes = storage.to_persisted_bytes().unwrap();
        let recovered = Storage::from_persisted_bytes(&bytes, &program).unwrap();
        assert_eq!(recovered, storage);
        assert_eq!(recovered.to_persisted_bytes().unwrap(), bytes);
        assert!(bytes.len() < MAX_STATE_BYTES);
    }

    #[test]
    fn corrupt_noncanonical_and_oversized_snapshots_fail_closed() {
        let program = fixture();
        let empty = Storage::default().to_persisted_bytes().unwrap();
        for bytes in [
            b"{}".as_slice(),
            b"not json",
            b"{\"version\":2,\"words\":{},\"strings\":{},\"maps\":[]}",
            b"{\"version\":1,\"words\":{},\"strings\":{},\"maps\":[],\"extra\":1}",
        ] {
            assert!(Storage::from_persisted_bytes(bytes, &program).is_err());
        }
        let mut noncanonical = empty.clone();
        noncanonical.push(b' ');
        assert!(Storage::from_persisted_bytes(&noncanonical, &program).is_err());
        assert!(Storage::from_persisted_bytes(&vec![b' '; MAX_STATE_BYTES + 1], &program).is_err());
        let mut unknown = Storage::default();
        unknown.values.insert("other".into(), word(1));
        assert!(
            Storage::from_persisted_bytes(&unknown.to_persisted_bytes().unwrap(), &program)
                .is_err()
        );
        let mut invalid_key = Storage::default();
        invalid_key.set_map_word("balances", vec![word(1), word(2)], word(3));
        assert!(Storage::from_persisted_bytes(
            &invalid_key.to_persisted_bytes().unwrap(),
            &program
        )
        .is_err());
        let mut duplicated: serde_json::Value = serde_json::from_slice(&empty).unwrap();
        duplicated["maps"] = serde_json::json!([
            ["balances", [word(9)], word(1)],
            ["balances", [word(9)], word(2)]
        ]);
        assert!(
            Storage::from_persisted_bytes(&serde_json::to_vec(&duplicated).unwrap(), &program)
                .is_err()
        );
    }
}
