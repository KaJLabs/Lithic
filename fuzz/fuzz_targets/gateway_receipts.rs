#![no_main]
use libfuzzer_sys::fuzz_target;
use lithovm_host::gateway_receipts::{decode, Receipt};

fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return;
    }
    if let Ok(receipt) = serde_json::from_slice::<Receipt>(data) {
        if let Ok(observed) = decode(
            &receipt,
            700777,
            "0x000000000000000000000000000000000000f000",
        ) {
            assert_eq!(observed["canonicalProofVerified"], false);
            assert!(observed["deployments"].as_array().unwrap().len() <= 4096);
            if observed["state"] == "failed" {
                assert_eq!(observed["deployments"], serde_json::json!([]));
                assert_eq!(observed["events"], serde_json::json!([]));
            }
        }
    }
});
