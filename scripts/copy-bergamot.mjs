// Copies the Bergamot WebAssembly translator (npm @browsermt/bergamot-translator,
// MPL-2.0) into public/bergamot so it is served — and embedded into the exe — as
// plain static files. The worker loads its .wasm relative to itself, so the
// files must keep their layout and must not go through the bundler.
import { cpSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = join(root, "node_modules", "@browsermt", "bergamot-translator");
const dst = join(root, "public", "bergamot");

rmSync(dst, { recursive: true, force: true });
mkdirSync(join(dst, "worker"), { recursive: true });
for (const file of [
  "translator.js",
  "worker/translator-worker.js",
  "worker/bergamot-translator-worker.js",
  "worker/bergamot-translator-worker.wasm",
]) {
  cpSync(join(src, file), join(dst, file));
}
console.log("bergamot → public/bergamot");
