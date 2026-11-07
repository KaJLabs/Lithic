#![no_main]
//! Malformed persisted records must fail closed at the real exported C ABI.
use libfuzzer_sys::fuzz_target;
use lithovm_ffi::{lithovm_execute_v1, lithovm_free_v1, Buffer};

unsafe extern "C" fn read(
    context: usize,
    key: *const u8,
    key_len: usize,
    out: *mut u8,
    capacity: usize,
) -> isize {
    if std::slice::from_raw_parts(key, key_len)
        != b"lithovm/v1/contracts/0000000000000000000000000000000000000001"
    {
        return -1;
    }
    let value = &*(context as *const &[u8]);
    if capacity < value.len() {
        return value.len() as isize;
    }
    std::ptr::copy_nonoverlapping(value.as_ptr(), out, value.len());
    value.len() as isize
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > 65536 {
        return;
    }
    let request = br#"{"version":1,"operation":"call","caller":"0x0000000000000000000000000000000000000009","contract":"0x0000000000000000000000000000000000000001","function":"value","arguments":"0x4c56414c02000107000101","nonce":1,"gas_limit":10000,"chain_id":700780,"block_height":1,"block_timestamp":100}"#;
    let mut output = Buffer {
        data: std::ptr::null_mut(),
        len: 0,
    };
    let status = unsafe {
        lithovm_execute_v1(
            request.as_ptr(),
            request.len(),
            &data as *const &[u8] as usize,
            Some(read),
            &mut output,
        )
    };
    assert_eq!(
        status, 0,
        "malformed state must produce a structured failure, not FFI panic/rejection"
    );
    assert!(!output.data.is_null() && output.len <= 2 * 1024 * 1024);
    let result: serde_json::Value =
        serde_json::from_slice(unsafe { std::slice::from_raw_parts(output.data, output.len) })
            .unwrap();
    unsafe { lithovm_free_v1(output) };
    if result["success"] == false {
        for name in ["writes", "deployments", "events"] {
            assert_eq!(result[name], serde_json::json!([]));
        }
    }
});
