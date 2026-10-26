#!/usr/bin/env node
// Fails if the app version differs between files (and the git tag, if given).
import { readFileSync } from "node:fs";

const read = (p) => readFileSync(new URL(`../${p}`, import.meta.url), "utf8");
const cargoVersion = (p) => /^version\s*=\s*"([^"]+)"/m.exec(read(p))?.[1];

const versions = {
  "package.json": JSON.parse(read("package.json")).version,
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  "src-tauri/Cargo.toml": cargoVersion("src-tauri/Cargo.toml"),
  "server/Cargo.toml": cargoVersion("server/Cargo.toml"),
};

const tag = process.argv[2];
if (tag) versions[`tag ${tag}`] = tag.replace(/^v/, "");

const unique = new Set(Object.values(versions));
for (const [file, v] of Object.entries(versions)) console.log(`${v ?? "MISSING"}  ${file}`);

if (unique.size !== 1 || unique.has(undefined)) {
  console.error("\nVersion mismatch: make every file agree before releasing.");
  process.exit(1);
}
console.log(`\nAll versions match: ${[...unique][0]}`);
