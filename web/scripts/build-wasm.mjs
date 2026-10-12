// Build the Rust core for the browser and generate JS bindings into src/wasm/.
// Two modules from one crate: `obj2cad_wasm` (parse, DXF, preview) and `obj2cad_wasm_dwg`
// (the same plus the DWG writer, several times larger, loaded only when DWG is chosen).
//
// Needs the wasm32 target (from rust-toolchain.toml) and wasm-bindgen-cli 0.2.129:
//   cargo install wasm-bindgen-cli --version 0.2.129 --locked
// or set WASM_BINDGEN to the path of a prebuilt binary.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const run = (cmd, args) => execFileSync(cmd, args, { cwd: root, stdio: "inherit" });

const wasm = "target/wasm32-unknown-unknown/release/obj2cad_wasm.wasm";
for (const [name, features] of [
  ["obj2cad_wasm", []],
  ["obj2cad_wasm_dwg", ["--features", "dwg"]],
]) {
  run("cargo", ["build", "--release", "--target", "wasm32-unknown-unknown", "-p", "obj2cad-wasm", ...features]);
  run(process.env.WASM_BINDGEN || "wasm-bindgen", [wasm, "--target", "web", "--out-dir", "web/src/wasm", "--out-name", name, "--typescript"]);
}
