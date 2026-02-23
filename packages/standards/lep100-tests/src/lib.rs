#[cfg(test)]
mod tests {
    use lithovm_receipts::{digest_v1, ReceiptV1};

    #[test]
    fn receipt_digest_is_stable() {
        let r = ReceiptV1 {
            request_id: "0x01".into(),
            contract_address: "0xabc".into(),
            provider_id: "provider1".into(),
            model_hash: "0x11".into(),
            input_hash: "0x22".into(),
            output_hash: "0x33".into(),
            cost_used: 7,
            timestamp: 1700000000,
            provider_signature: "0xsig".into(),
        };
        let d1 = digest_v1(&r);
        let d2 = digest_v1(&r);
        assert_eq!(d1, d2);
    }
}
