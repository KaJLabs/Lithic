import express from "express";
import crypto from "crypto";
import { loadOrCreateKeypair } from "./keys.js";
import { digestReceiptV1, hex, signReceiptV1, sha256 } from "./receipt.js";

const app = express();
app.use(express.json({ limit: "2mb" }));

const kp = loadOrCreateKeypair();

const PROVIDER_ID = process.env.PROVIDER_ID || "provider-ref-001";

app.get("/health", (_req, res) => {
  res.json({ ok: true, provider_id: PROVIDER_ID });
});

// LEP100-2 canonical request handler (scaffold)
app.post("/v1/ai/execute", async (req, res) => {
  const body = req.body || {};
  const request_id = body.request_id || crypto.randomBytes(16).toString("hex");
  const contract_address = body.contract_address || "0x0";
  const model_hash = body.model_hash || "0xmodel";
  const input_hash = body.input_hash || hex(sha256(Buffer.from(JSON.stringify(body.parameters || {}))));
  const cost_ceiling = body.cost_ceiling || "10 LITHO";

  // "AI output" (scaffold): echo prompt summary-like
  const prompt = (body.parameters && body.parameters.prompt) ? String(body.parameters.prompt) : "";
  const output = prompt.length > 120 ? prompt.slice(0, 120) + "…" : prompt;
  const output_hash = hex(sha256(Buffer.from(output, "utf8")));

  // cost_used (scaffold): proportional to output length
  const cost_used = Math.min(output.length, 50); // numeric units (replace with LITHO pricing logic)

  const receipt = {
    request_id: typeof request_id === "string" ? request_id : String(request_id),
    contract_address,
    provider_id: PROVIDER_ID,
    model_hash,
    input_hash,
    output_hash,
    cost_used,
    timestamp: Math.floor(Date.now() / 1000),
  };

  const provider_signature = signReceiptV1(receipt, kp.secretKey);

  res.json({
    ...receipt,
    output,
    cost_ceiling,
    provider_pubkey_b64: Buffer.from(kp.publicKey).toString("base64"),
    provider_signature,
    receipt_digest: hex(digestReceiptV1(receipt)),
    proof_system: body.proof_system || null,
    verification_key_id: body.verification_key_id || null,
    zk_proof: body.zk_required ? "stub_proof_bytes" : null,
  });
});

const port = Number(process.env.PORT || 8787);
app.listen(port, () => {
  console.log(`[provider] ${PROVIDER_ID} listening on :${port}`);
  console.log(`[provider] publicKey(base64)=${Buffer.from(kp.publicKey).toString("base64")}`);
});
