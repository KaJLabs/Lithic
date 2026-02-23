import nacl from "tweetnacl";
import { encodeBase64, decodeBase64 } from "tweetnacl-util";
import fs from "fs";
import path from "path";

const KEY_PATH = path.resolve(process.cwd(), "provider-key.json");

export function loadOrCreateKeypair() {
  if (fs.existsSync(KEY_PATH)) {
    const obj = JSON.parse(fs.readFileSync(KEY_PATH, "utf8"));
    return {
      publicKey: decodeBase64(obj.publicKey),
      secretKey: decodeBase64(obj.secretKey),
    };
  }
  const kp = nacl.sign.keyPair();
  fs.writeFileSync(KEY_PATH, JSON.stringify({
    publicKey: encodeBase64(kp.publicKey),
    secretKey: encodeBase64(kp.secretKey),
  }, null, 2));
  return kp;
}
