#![no_main]

use libfuzzer_sys::fuzz_target;
use lithovm_bytecode::parse;

fuzz_target!(|data: &[u8]| {
    if let Ok(values) = lithovm_bytecode::values::decode(data) {
        let encoded = lithovm_bytecode::values::encode(&values).unwrap();
        assert_eq!(encoded, data);
    }
    if let Ok(program) = parse(data) {
        let canonical = program.encode().expect("a decoded program must re-encode");
        let reparsed = parse(&canonical).expect("canonical bytecode must decode");
        assert_eq!(reparsed, program);
        assert_eq!(reparsed.encode().unwrap(), canonical);
    }
});
