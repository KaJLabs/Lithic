#![no_main]

use libfuzzer_sys::fuzz_target;
use lithic_lithovm::compile;
use lithovm::Vm;

const MAX_SOURCE_BYTES: usize = 64 * 1024;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_SOURCE_BYTES {
        return;
    }
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(artifact) = compile(source) {
        assert_eq!(
            artifact.target,
            format!("lithovm-native-v{}", artifact.bytecode_version)
        );
        assert_eq!(compile(source).unwrap(), artifact);
        let bytes = hex::decode(
            artifact
                .bytecode
                .strip_prefix("0x")
                .expect("compiler bytecode must use a 0x prefix"),
        )
        .expect("compiler bytecode must be hex");
        assert_eq!(bytes[7], artifact.bytecode_version);
        Vm::default()
            .load_and_validate(&bytes)
            .expect("compiler output must load in the runtime");
    }
});
