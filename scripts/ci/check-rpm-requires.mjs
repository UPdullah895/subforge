#!/usr/bin/env node
// Checks that a built .rpm declares every dependency from tauri.conf.json
// (bundle.linux.rpm.depends), including the rich ones such as `(ffmpeg or ffmpeg-free)`.
//
//   node scripts/ci/check-rpm-requires.mjs <file.rpm>
//
// Runs `rpm -qpR` (only reads the package; nothing is installed). Exit 0 ok, 1 missing, 2 usage.
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CONF = new URL("../../src-tauri/tauri.conf.json", import.meta.url);

/** `bundle.linux.rpm.depends` from tauri.conf.json. */
export function configuredDepends() {
  return JSON.parse(readFileSync(CONF, "utf8")).bundle.linux.rpm.depends;
}

/**
 * The configured dependencies that do not appear as a whole line of `rpm -qpR` output.
 * @param {string} rpmOutput
 * @param {string[]} depends
 * @returns {string[]}
 */
export function missingRequires(rpmOutput, depends) {
  const lines = new Set(rpmOutput.split(/\r?\n/).map((l) => l.trim()));
  return depends.filter((d) => !lines.has(d));
}

function main(argv) {
  if (argv.length !== 1) {
    console.error("usage: node scripts/ci/check-rpm-requires.mjs <file.rpm>");
    return 2;
  }
  const res = spawnSync("rpm", ["-qpR", argv[0]], { encoding: "utf8" });
  if (res.error || res.status !== 0) {
    console.error(`FAIL: rpm -qpR ${argv[0]} failed: ${res.error?.message ?? res.stderr}`);
    return 1;
  }
  console.log(res.stdout.trimEnd());
  const missing = missingRequires(res.stdout, configuredDepends());
  if (missing.length) {
    for (const m of missing) console.error(`FAIL: the rpm does not require ${m}`);
    return 1;
  }
  console.log("\nAll configured rpm dependencies are declared.");
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
