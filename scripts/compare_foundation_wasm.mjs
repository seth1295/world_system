import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const [wasmPath, nativePath] = process.argv.slice(2);
if (!wasmPath || !nativePath) {
  throw new Error("usage: node scripts/compare_foundation_wasm.mjs <wasm-file> <native-output>");
}

const wasm = readFileSync(wasmPath);
const native = readFileSync(nativePath);
const vector = JSON.parse(readFileSync(resolve("conformance/vectors/topology/stage2-parity.json"), "utf8"));
const { instance } = await WebAssembly.instantiate(wasm, {});
const exportFunction = instance.exports.veyra_capi_foundation_parity_payload;
const memory = instance.exports.memory;
if (typeof exportFunction !== "function" || !(memory instanceof WebAssembly.Memory)) {
  throw new Error("WASM foundation parity exports are missing");
}
const packed = exportFunction();
if (packed === 0n) throw new Error("WASM foundation vector generation failed");
const pointer = Number(packed & 0xffff_ffffn);
const length = Number((packed >> 32n) & 0xffff_ffffn);
const wasmOutput = Buffer.from(memory.buffer, pointer, length);
if (!wasmOutput.equals(native)) {
  let offset = 0;
  const shared = Math.min(wasmOutput.length, native.length);
  while (offset < shared && wasmOutput[offset] === native[offset]) offset += 1;
  throw new Error(`native/WASM mismatch at byte ${offset}: native=${native.length} bytes wasm=${wasmOutput.length} bytes`);
}
const digest = createHash("sha256").update(native).digest("hex");
console.log(`PASS native/WASM byte parity: ${native.length} bytes; sha256=${digest}; ${vector.sample_count} deterministic topology cases`);
