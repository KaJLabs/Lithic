import http from "http";

function postJson(path, data) {
  return new Promise((resolve, reject) => {
    const body = Buffer.from(JSON.stringify(data));
    const req = http.request({
      hostname: "127.0.0.1",
      port: process.env.PORT || 8787,
      path,
      method: "POST",
      headers: {
        "content-type": "application/json",
        "content-length": body.length,
      }
    }, (res) => {
      let chunks = "";
      res.on("data", d => chunks += d);
      res.on("end", () => resolve({ status: res.statusCode, body: JSON.parse(chunks) }));
    });
    req.on("error", reject);
    req.write(body);
    req.end();
  });
}

(async () => {
  console.log("[selftest] run provider then execute:");
  console.log("  node src/server.js");
  console.log("  node src/selftest.js");
  try {
    const r = await postJson("/v1/ai/execute", {
      request_id: "0xabc123",
      contract_address: "0xdeadbeef",
      model_hash: "0xmodelhash",
      parameters: { prompt: "Hello Lithic provider" },
      cost_ceiling: "10 LITHO",
    });
    console.log("[selftest] status:", r.status);
    console.log("[selftest] receipt_digest:", r.body.receipt_digest);
    process.exit(0);
  } catch (e) {
    console.error("[selftest] failed:", e.message);
    process.exit(1);
  }
})();
