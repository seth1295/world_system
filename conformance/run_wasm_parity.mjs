import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repositoryRoot = fileURLToPath(new URL("../", import.meta.url));
const [adapterPath] = process.argv.slice(2);

if (!adapterPath) {
  throw new Error(
    "Pass a Stage 10 WASM adapter module exporting async runQueries({worldRoot, queryPath}).",
  );
}

const adapterUrl = pathToFileURL(resolve(adapterPath));
const adapter = await import(adapterUrl.href);
if (typeof adapter.runQueries !== "function") {
  throw new Error("The WASM adapter must export async runQueries({worldRoot, queryPath}).");
}

const expectedPath = resolve(repositoryRoot, "conformance/expected/stage4.jsonl");
const actual = await adapter.runQueries({
  worldRoot: resolve(repositoryRoot, "conformance/worlds"),
  queryPath: resolve(repositoryRoot, "conformance/queries/stage4.jsonl"),
});
const actualBytes = typeof actual === "string" ? new TextEncoder().encode(actual) : actual;
if (!(actualBytes instanceof Uint8Array)) {
  throw new Error("runQueries must return a string or Uint8Array containing JSONL output.");
}

const expectedBytes = await readFile(expectedPath);
if (Buffer.compare(Buffer.from(actualBytes), expectedBytes) !== 0) {
  throw new Error("WASM query output differs from conformance/expected/stage4.jsonl.");
}
process.stdout.write(`PASS ${actualBytes.byteLength} bytes match stage4.jsonl\n`);
