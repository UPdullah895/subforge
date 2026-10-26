#!/usr/bin/env node
// Fails if the app version differs between files (and the git tag, if given).
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const cargoVersion = (text) => /^version\s*=\s*"([^"]+)"/m.exec(text)?.[1];

/** Version of package `name` in a Cargo.lock. */
export function lockVersion(text, name) {
  const re = new RegExp(`\\[\\[package\\]\\]\\s*\\nname = "${name.replace(/[-]/g, "\\-")}"\\s*\\nversion = "([^"]+)"`);
  return re.exec(text)?.[1];
}

/** `pkgver=` of an AUR PKGBUILD. */
export function pkgbuildVersion(text) {
  return /^pkgver=([^\s#]+)/m.exec(text)?.[1];
}

/** Every place that carries the app version, read through `read(path)`. */
export function collectVersions(read) {
  const pkgLock = JSON.parse(read("package-lock.json"));
  return {
    "package.json": JSON.parse(read("package.json")).version,
    "package-lock.json": pkgLock.version,
    'package-lock.json packages[""]': pkgLock.packages?.[""]?.version,
    "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
    "src-tauri/Cargo.toml": cargoVersion(read("src-tauri/Cargo.toml")),
    "src-tauri/Cargo.lock": lockVersion(read("src-tauri/Cargo.lock"), "subforge"),
    "server/Cargo.toml": cargoVersion(read("server/Cargo.toml")),
    "server/Cargo.lock": lockVersion(read("server/Cargo.lock"), "subforge-server"),
    "packaging/aur/subforge/PKGBUILD": pkgbuildVersion(read("packaging/aur/subforge/PKGBUILD")),
    "packaging/aur/subforge-bin/PKGBUILD": pkgbuildVersion(read("packaging/aur/subforge-bin/PKGBUILD")),
  };
}

/** Returns the single shared version, or null when they disagree / one is missing. */
export function agreedVersion(versions) {
  const unique = new Set(Object.values(versions));
  return unique.size === 1 && !unique.has(undefined) ? [...unique][0] : null;
}

function main() {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..");
  const read = (p) => readFileSync(join(root, p), "utf8");
  const versions = collectVersions(read);
  const tag = process.argv[2];
  if (tag) versions[`tag ${tag}`] = tag.replace(/^v/, "");

  for (const [file, v] of Object.entries(versions)) console.log(`${v ?? "MISSING"}  ${file}`);
  const agreed = agreedVersion(versions);
  if (agreed === null) {
    console.error("\nVersion mismatch: make every file agree before releasing.");
    process.exit(1);
  }
  console.log(`\nAll versions match: ${agreed}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main();
