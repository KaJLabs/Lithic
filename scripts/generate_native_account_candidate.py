#!/usr/bin/env python3
"""Generate the unapproved native-account engineering fixture.

Shared source fragments keep hash/admission checks identical at each entrypoint.
This is not the LEP100-15 reference implementation or a deployment script.
"""
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DESTINATION = ROOT / "sdk/contracts/standards/lep10015_native_account_candidate.lithic"


def transaction_checks():
    return f"""
        require(512 < bytes_len(transaction) + 1);
        require(word_u64(bytes_word(bytes_slice(transaction, 0, 32))) == 32);
        let account: address = word_address(bytes_word(bytes_slice(transaction, 32, 32)));
        require(account == contract.address);
        require(word_u64(bytes_word(bytes_slice(transaction, 64, 32))) == chain.id);
        let nonce: u256 = word_u256(bytes_word(bytes_slice(transaction, 96, 32)));
        require(word_u64(bytes_word(bytes_slice(transaction, 128, 32))) == 256);
        let valid_after: u64 = word_u64(bytes_word(bytes_slice(transaction, 160, 32)));
        let valid_until: u64 = word_u64(bytes_word(bytes_slice(transaction, 192, 32)));
        if valid_until == 0 {{ }} else {{ require((valid_until < valid_after) == false); }}
        let policy: bytes32 = bytes_word(bytes_slice(transaction, 224, 32));
        let metadata: bytes32 = bytes_word(bytes_slice(transaction, 256, 32));
        let count: u64 = word_u64(bytes_word(bytes_slice(transaction, 288, 32)));
        require(0 < count); require(count < 17);
        let mut hashes: bytes = bytes_slice(transaction, 0, 0);
        let mut cursor: u64 = count * 32;
        let mut index: u64 = 0;
        repeat count {{
            require(word_u64(bytes_word(bytes_slice(transaction, 320 + index * 32, 32))) == cursor);
            let base: u64 = 320 + cursor;
            let target: address = word_address(bytes_word(bytes_slice(transaction, base, 32)));
            require((target == ZERO_ADDRESS) == false);
            let amount: u256 = word_u256(bytes_word(bytes_slice(transaction, base + 32, 32)));
            require(word_u64(bytes_word(bytes_slice(transaction, base + 64, 32))) == 128);
            let operation: u64 = word_u64(bytes_word(bytes_slice(transaction, base + 96, 32)));
            require(operation == 0);
            let size: u64 = word_u64(bytes_word(bytes_slice(transaction, base + 128, 32)));
            require(size < 4097);
            let data: bytes = bytes_slice(transaction, base + 160, size);
            let padded: u64 = ((size + 31) / 32) * 32;
            let mut pad: u64 = size;
            repeat padded - size {{ require(bytes_get(transaction, base + 160 + pad) == 0); pad = pad + 1; }}
            let encoded: bytes = bytes_concat(bytes_concat(bytes_concat(bytes_concat(word_bytes(CALL_TYPE), word_bytes(target)), word_bytes(amount)), word_bytes(keccak256(data))), word_bytes(operation));
            hashes = bytes_concat(hashes, word_bytes(keccak256(encoded)));
            cursor = cursor + 160 + padded; index = index + 1;
        }}
        require(bytes_len(transaction) == 320 + cursor);
        let domain: bytes32 = keccak256(bytes_concat(bytes_concat(bytes_concat(bytes_concat(word_bytes(DOMAIN_TYPE), word_bytes(DOMAIN_NAME)), word_bytes(DOMAIN_VERSION)), word_bytes(chain.id)), word_bytes(contract.address)));
        let encoded_tx: bytes = bytes_concat(bytes_concat(bytes_concat(bytes_concat(bytes_concat(bytes_concat(bytes_concat(bytes_concat(word_bytes(TX_TYPE), word_bytes(account)), word_bytes(chain.id)), word_bytes(nonce)), word_bytes(keccak256(hashes))), word_bytes(valid_after)), word_bytes(valid_until)), word_bytes(policy)), word_bytes(metadata));
        let prefix: bytes = bytes_slice(word_bytes(6401), 30, 2);
        let txid: bytes32 = keccak256(bytes_concat(bytes_concat(prefix, word_bytes(domain)), word_bytes(keccak256(encoded_tx))));
    """


POLICY_CHECK = """
        let current_policy: bytes32 = keccak256(bytes_concat(bytes_concat(word_bytes(POLICY_TAG), word_bytes(self.auth_epoch)), word_bytes(ZERO_HASH)));
        require(policy == current_policy);
        if valid_until == 0 { } else { require((valid_until < block.timestamp) == false); }
"""

PROPOSAL_CHECK = """
        require(self.initialized);
        require(self.proposed[txid]);
        require(self.executed[txid] == false);
        require(self.cancelled[txid] == false);
        require(self.proposal_epoch[txid] == self.auth_epoch);
        if self.expiry[txid] == 0 { } else { require((self.expiry[txid] < block.timestamp) == false); }
        let signer_id: bytes32 = self.signer_ids[msg.sender];
        require((signer_id == ZERO_HASH) == false);
"""

NEW_SIGNER = """
    require((signer == ZERO_ADDRESS) == false);
    require(0 < scheme); require(scheme < 3);
    require(self.signer_ids[signer] == ZERO_HASH);
    self.registration_sequence = self.registration_sequence + 1;
    let id: bytes32 = keccak256(bytes_concat(bytes_concat(word_bytes(SIGNER_TAG), word_bytes(contract.address)), word_bytes(self.registration_sequence)));
    require((id == ZERO_HASH) == false); require(self.signers[id] == ZERO_ADDRESS);
    self.signer_ids[signer] = id; self.signers[id] = signer; self.schemes[id] = scheme;
"""

ADMINS = [
    ("set_threshold", "u64", [1], [("next", "u64", 40)], """
        require(0 < next); require(next < self.signer_count + 1);
        self.threshold = next; self.auth_epoch = self.auth_epoch + 1;
        emit ThresholdChanged { threshold: next };
    """),
    ("add_signer", "address,u64", [4, 1], [("signer", "address", 40), ("scheme", "u64", 73)], """
        require(self.signer_count < 16);
    """ + NEW_SIGNER + """
        self.signer_index[self.signer_count] = id; self.signer_count = self.signer_count + 1;
        self.auth_epoch = self.auth_epoch + 1; emit SignerAdded { signer: id };
    """),
    ("remove_signer", "bytes32", [5], [("id", "bytes32", 40)], """
        let signer: address = self.signers[id]; require((signer == ZERO_ADDRESS) == false);
        require(self.threshold < self.signer_count);
        let mut admin_index: u64 = 0; let mut found: bool = false;
        repeat self.signer_count {
            if self.signer_index[admin_index] == id { self.signer_index[admin_index] = self.signer_index[self.signer_count - 1]; found = true; }
            admin_index = admin_index + 1;
        }
        require(found); self.signer_count = self.signer_count - 1; self.signer_index[self.signer_count] = ZERO_HASH;
        self.signers[id] = ZERO_ADDRESS; self.signer_ids[signer] = ZERO_HASH; self.schemes[id] = 0;
        self.auth_epoch = self.auth_epoch + 1; emit SignerRemoved { signer: id };
    """),
    ("replace_signer", "bytes32,address,u64", [5, 4, 1], [("old_id", "bytes32", 40), ("replacement", "address", 73), ("scheme", "u64", 106)], """
        let old: address = self.signers[old_id]; require((old == ZERO_ADDRESS) == false);
        require((replacement == ZERO_ADDRESS) == false); require(0 < scheme); require(scheme < 3);
        if replacement == old { } else { require(self.signer_ids[replacement] == ZERO_HASH); }
        self.registration_sequence = self.registration_sequence + 1;
        let id: bytes32 = keccak256(bytes_concat(bytes_concat(word_bytes(SIGNER_TAG), word_bytes(contract.address)), word_bytes(self.registration_sequence)));
        require((id == ZERO_HASH) == false); require(self.signers[id] == ZERO_ADDRESS);
        let mut admin_index: u64 = 0; let mut found: bool = false;
        repeat self.signer_count { if self.signer_index[admin_index] == old_id { self.signer_index[admin_index] = id; found = true; } admin_index = admin_index + 1; }
        require(found); self.signers[old_id] = ZERO_ADDRESS; self.signer_ids[old] = ZERO_HASH; self.schemes[old_id] = 0;
        self.signers[id] = replacement; self.signer_ids[replacement] = id; self.schemes[id] = scheme;
        self.auth_epoch = self.auth_epoch + 1;
        emit SignerRemoved { signer: old_id }; emit SignerAdded { signer: id };
    """),
    ("cancel", "bytes32", [5], [("id", "bytes32", 40)], """
        require(self.proposed[id]); require(self.executed[id] == false); require(self.cancelled[id] == false);
        self.cancelled[id] = true; emit Cancelled { txid: id };
    """),
]


def admin_constants():
    return "\n".join(f'    const ADMIN_{name.upper()}: bytes32 = keccak256("{name}({signature})");' for name, signature, *_ in ADMINS)


def admin_dispatch():
    output = "revert();"
    for name, _, tags, arguments, body in reversed(ADMINS):
        prefix = int.from_bytes(b"LVAL\x01" + len(tags).to_bytes(2, "big") + bytes(tags[:1]), "big")
        # Compare the exact 8-byte LVAL header/first tag without treating it as a word.
        checks = f"require(bytes_len(data) == {39 + 33 * len(tags)}); require(bytes_slice(data, 32, 8) == bytes_slice(word_bytes({prefix}), 24, 8));"
        for index, tag in enumerate(tags[1:], 1):
            checks += f"require(bytes_get(data, {39 + 33 * index}) == {tag});"
        for arg, ty, offset in arguments:
            expr = f"bytes_word(bytes_slice(data, {offset}, 32))"
            if ty in ("u64", "address"):
                expr = f"word_{ty}({expr})"
            checks += f"let {arg}: {ty} = {expr};"
        output = f"if selector == ADMIN_{name.upper()} {{ {checks} {body} }} else {{ {output} }}"
    return "let selector: bytes32 = bytes_word(bytes_slice(data, 0, 32));" + output


def public_admins():
    result = []
    for name, _, _, arguments, body in ADMINS:
        parameters = ", ".join(f"{arg}: {ty}" for arg, ty, _ in arguments)
        result.append(f"pub fn {name}({parameters}) -> bool {{ require(msg.sender == contract.address); {body} return true; }}")
    return "\n".join(result)


def source():
    return '''// GENERATED by scripts/generate_native_account_candidate.py.
// PROPOSED, UNAPPROVED ENGINEERING FIXTURE. NATIVE CALLS ONLY.
// Not complete LEP100-15 conformance; never use this fixture for custody.
contract NativeAccountCandidate {
    const ZERO_HASH: bytes32 = 0x0000000000000000000000000000000000000000000000000000000000000000;
    const ZERO_ADDRESS: address = 0x0000000000000000000000000000000000000000;
    const ZERO_AMOUNT: u256 = 0;
    const DOMAIN_TYPE: bytes32 = keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");
    const DOMAIN_NAME: bytes32 = keccak256("LEP100-15");
    const DOMAIN_VERSION: bytes32 = keccak256("1");
    const CALL_TYPE: bytes32 = keccak256("Call(address target,uint256 value,bytes data,uint8 operation)");
    const TX_TYPE: bytes32 = keccak256("MultiSigTransaction(address account,uint256 chainId,uint256 nonce,Call[] calls,uint64 validAfter,uint64 validUntil,bytes32 policyHash,bytes32 metadataHash)Call(address target,uint256 value,bytes data,uint8 operation)");
    const POLICY_TAG: bytes32 = keccak256("LEP100-15 AuthorizationPolicy v1");
    const SIGNER_TAG: bytes32 = keccak256("LEP100-15 SignerId v1");
    const VALIDATOR_SELECTOR: bytes32 = keccak256("is_valid_signature(bytes32,bytes)");
    const SIGNATURE_PREFIX: u64 = 5500655789586514437;
    const TRUE_RETURN_PREFIX: u64 = 5500655789569736963;
''' + admin_constants() + '''
    state {
        initialized: bool; signer_count: u64; threshold: u64;
        auth_epoch: u256; next_nonce: u256; registration_sequence: u256;
        signers: map<bytes32, address>; signer_ids: map<address, bytes32>; schemes: map<bytes32, u64>;
        signer_index: map<u64, bytes32>;
        proposed: map<bytes32, bool>; proposal_nonce: map<bytes32, u256>;
        proposal_epoch: map<bytes32, u256>; expiry: map<bytes32, u64>;
        executed: map<bytes32, bool>; cancelled: map<bytes32, bool>;
        approvals: map<bytes32, map<bytes32, bool>>;
        revoked: map<bytes32, map<bytes32, bool>>;
    }
    event Proposed { txid: bytes32 }
    event Approval { txid: bytes32, signer: bytes32, approved: bool }
    event Executed { txid: bytes32 }
    event ThresholdChanged { threshold: u64 }
    event SignerAdded { signer: bytes32 }
    event SignerRemoved { signer: bytes32 }
    event Cancelled { txid: bytes32 }
    pub fn initialize(signer_list: bytes, threshold: u64) -> bool {
        require(self.initialized == false);
        require(word_u64(bytes_word(bytes_slice(signer_list, 0, 32))) == 32);
        let count: u64 = word_u64(bytes_word(bytes_slice(signer_list, 32, 32)));
        require(0 < count); require(count < 17); require(0 < threshold); require(threshold < count + 1);
        require(bytes_len(signer_list) == 64 + count * 64);
        let mut index: u64 = 0;
        repeat count {
            let signer: address = word_address(bytes_word(bytes_slice(signer_list, 64 + index * 64, 32)));
            let scheme: u64 = word_u64(bytes_word(bytes_slice(signer_list, 96 + index * 64, 32)));
            require(0 < scheme); require(scheme < 3);
            require((signer == ZERO_ADDRESS) == false); require(self.signer_ids[signer] == ZERO_HASH);
            let id: bytes32 = keccak256(bytes_concat(bytes_concat(word_bytes(SIGNER_TAG), word_bytes(contract.address)), word_bytes(index + 1)));
            require((id == ZERO_HASH) == false); require(self.signers[id] == ZERO_ADDRESS);
            self.signer_ids[signer] = id; self.signers[id] = signer; self.signer_index[index] = id; self.schemes[id] = scheme;
            index = index + 1;
        }
        self.signer_count = count; self.threshold = threshold; self.auth_epoch = 1; self.initialized = true;
        self.registration_sequence = word_u256(bytes_word(word_bytes(count)));
        return true;
    }
    pub fn nonce() -> u256 { return self.next_nonce; }
    pub fn threshold() -> u64 { return self.threshold; }
    pub fn epoch() -> u256 { return self.auth_epoch; }
    pub fn signer_count() -> u64 { return self.signer_count; }
    pub fn signer_id(signer: address) -> bytes32 { return self.signer_ids[signer]; }
    pub fn policy_hash() -> bytes32 {
        return keccak256(bytes_concat(bytes_concat(word_bytes(POLICY_TAG), word_bytes(self.auth_epoch)), word_bytes(ZERO_HASH)));
    }
    pub fn hash_transaction(transaction: bytes) -> bytes32 {
''' + transaction_checks() + '''
        return txid;
    }
    pub fn propose(transaction: bytes) -> bytes32 {
        require(self.initialized);
        require((self.signer_ids[msg.sender] == ZERO_HASH) == false);
''' + transaction_checks() + POLICY_CHECK + '''
        require(nonce == self.next_nonce);
        require(self.proposed[txid] == false); require(self.executed[txid] == false);
        self.proposed[txid] = true; self.proposal_nonce[txid] = nonce;
        self.proposal_epoch[txid] = self.auth_epoch; self.expiry[txid] = valid_until;
        self.next_nonce = self.next_nonce + 1;
        emit Proposed { txid: txid }; return txid;
    }
    pub fn approve(txid: bytes32) -> bool {
''' + PROPOSAL_CHECK + '''
        self.approvals[txid][signer_id] = true;
        emit Approval { txid: txid, signer: signer_id, approved: true }; return true;
    }
    pub fn revoke(txid: bytes32) -> bool {
''' + PROPOSAL_CHECK + '''
        self.approvals[txid][signer_id] = false; self.revoked[txid][signer_id] = true;
        emit Approval { txid: txid, signer: signer_id, approved: false }; return true;
    }
    pub fn execute(transaction: bytes, signatures: bytes) -> bool {
        require(self.initialized);
''' + transaction_checks() + POLICY_CHECK + '''
        require(self.executed[txid] == false);
        require(self.cancelled[txid] == false);
        require((block.timestamp < valid_after) == false);
        if self.proposed[txid] {
            require(self.proposal_nonce[txid] == nonce); require(self.proposal_epoch[txid] == self.auth_epoch);
        } else { require(nonce == self.next_nonce); }
        let mut votes: u64 = 0; let mut voter: u64 = 0;
        repeat self.signer_count {
            let id: bytes32 = self.signer_index[voter];
            if self.approvals[txid][id] { votes = votes + 1; }
            voter = voter + 1;
        }
        require(word_u64(bytes_word(bytes_slice(signatures, 0, 32))) == 32);
        let signature_count: u64 = word_u64(bytes_word(bytes_slice(signatures, 32, 32)));
        require(signature_count < 17);
        let mut sig_cursor: u64 = signature_count * 32;
        let mut sig_index: u64 = 0; let mut previous: bytes32 = ZERO_HASH;
        repeat signature_count {
            require(word_u64(bytes_word(bytes_slice(signatures, 64 + sig_index * 32, 32))) == sig_cursor);
            let base: u64 = 64 + sig_cursor;
            let id: bytes32 = bytes_word(bytes_slice(signatures, base, 32));
            require(word_u256(id) >= word_u256(previous)); require((id == previous) == false);
            let scheme: u64 = word_u64(bytes_word(bytes_slice(signatures, base + 32, 32)));
            require(scheme == self.schemes[id]);
            require(word_u64(bytes_word(bytes_slice(signatures, base + 64, 32))) == 96);
            let signature_size: u64 = word_u64(bytes_word(bytes_slice(signatures, base + 96, 32)));
            require(signature_size < 513);
            let signature: bytes = bytes_slice(signatures, base + 128, signature_size);
            let padded: u64 = ((signature_size + 31) / 32) * 32;
            let mut pad: u64 = signature_size;
            repeat padded - signature_size { require(bytes_get(signatures, base + 128 + pad) == 0); pad = pad + 1; }
            let signer: address = self.signers[id]; require((signer == ZERO_ADDRESS) == false);
            require(self.revoked[txid][id] == false);
            if scheme == 1 { require(secp256k1_verify(txid, signature, signer)); } else {
                require(scheme == 2);
                let header: bytes = bytes_slice(word_bytes(SIGNATURE_PREFIX), 24, 8);
                let value_tag: bytes = bytes_slice(word_bytes(7), 31, 1);
                let size_bytes: bytes = bytes_slice(word_bytes(signature_size), 30, 2);
                let args: bytes = bytes_concat(bytes_concat(bytes_concat(bytes_concat(header, word_bytes(txid)), value_tag), size_bytes), signature);
                let payload: bytes = bytes_concat(word_bytes(VALIDATOR_SELECTOR), args);
                let result: bytes = invoke_static_payload(signer, 0, payload);
                let expected: bytes = bytes_concat(bytes_slice(word_bytes(TRUE_RETURN_PREFIX), 24, 8), word_bytes(true));
                require(result == expected);
            }
            if self.approvals[txid][id] { } else { votes = votes + 1; }
            previous = id; sig_cursor = sig_cursor + 128 + padded; sig_index = sig_index + 1;
        }
        require(bytes_len(signatures) == 64 + sig_cursor);
        require((votes < self.threshold) == false);
        self.executed[txid] = true;
        if self.proposed[txid] { } else { self.next_nonce = self.next_nonce + 1; }
        let mut call_index: u64 = 0;
        repeat count {
            let offset: u64 = word_u64(bytes_word(bytes_slice(transaction, 320 + call_index * 32, 32)));
            let target: address = word_address(bytes_word(bytes_slice(transaction, 320 + offset, 32)));
            let amount: u256 = word_u256(bytes_word(bytes_slice(transaction, 352 + offset, 32)));
            let size: u64 = word_u64(bytes_word(bytes_slice(transaction, 448 + offset, 32)));
            let data: bytes = bytes_slice(transaction, 480 + offset, size);
            if size == 0 { transfer_native(target, amount); } else {
                if target == contract.address {
                    require(amount == ZERO_AMOUNT);
''' + admin_dispatch() + '''
                } else { let result: bytes = invoke_payload(target, amount, data); }
            }
            call_index = call_index + 1;
        }
        emit Executed { txid: txid }; return true;
    }
''' + public_admins() + '''
}
'''


if __name__ == "__main__":
    DESTINATION.write_text(source(), encoding="utf-8", newline="\n")
