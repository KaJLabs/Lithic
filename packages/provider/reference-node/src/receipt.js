import crypto from "crypto";
import nacl from "tweetnacl";
import { encodeBase64 } from "tweetnacl-util";

export const DOMAIN = "LITHOSPHERE_AI_RECEIPT_V1";

export function sha256(bytes) {
  return crypto.createHash("sha256").update(bytes).digest();
}

export function hex(bytes) {
  return "0x" + Buffer.from(bytes).toString("hex");
}

export function digestReceiptV1(r) {
  // Scaffold canonicalization: field concatenation with separators.
  // Production: binary canonical encoding per LEP100-4.
  const s = [
    DOMAIN,
    r.request_id,
    r.contract_address,
    r.provider_id,
    r.model_hash,
    r.input_hash,
    r.output_hash,
    String(r.cost_used),
    String(r.timestamp),
  ].join("|");
  return sha256(Buffer.from(s, "utf8"));
}

export function signReceiptV1(receipt, secretKey) {
  const digest = digestReceiptV1(receipt);
  const sig = nacl.sign.detached(digest, secretKey);
  return encodeBase64(sig);
}
