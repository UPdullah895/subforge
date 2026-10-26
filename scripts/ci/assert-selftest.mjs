#!/usr/bin/env node
// Checks the result of `subforge --self-test` in CI (both the Windows and the Linux e2e legs).
//
//   node scripts/ci/assert-selftest.mjs <selftest.json> --exit-code <n> [--expect-device <device>]
//
// Fails (exit 1) unless the self-test process exited 0, the report says ok, every step in
// STEP_NAMES is present and ok, and (when given) `device_used` equals the expected device.
// Exit 2 means bad arguments. Uses only Node built-ins.
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Must match `subforge_core::selftest::STEP_NAMES` (a test keeps them in sync). */
export const STEP_NAMES = [
  "tools_bundled",
  "prepare_fixtures",
  "list_tracks",
  "extract_subtitles",
  "extract_audio",
  "convert_ass_srt_vtt",
  "shift_stretch",
  "translate_stub",
  "whisper_tiny",
  "mux",
  "verify_mux",
];

/**
 * Returns the list of problems with a self-test run (empty when it passed).
 * @param {any} report parsed selftest.json
 * @param {{ exitCode: number, expectDevice?: string }} opts
 * @returns {string[]}
 */
export function checkSelfTest(report, { exitCode, expectDevice }) {
  const failures = [];
  if (exitCode !== 0) failures.push(`the self-test process ended with exit code ${exitCode}`);
  if (!report || typeof report !== "object" || !Array.isArray(report.steps)) {
    failures.push('the report has no "steps" array');
    return failures;
  }
  if (report.ok !== true) failures.push('the report\'s "ok" is false');
  for (const name of STEP_NAMES) {
    const step = report.steps.find((s) => s && s.name === name);
    if (!step) failures.push(`missing step ${name}`);
    else if (step.ok !== true) failures.push(`step ${name} failed: ${step.detail}`);
  }
  if (expectDevice !== undefined && report.device_used !== expectDevice) {
    failures.push(
      `device_used is ${JSON.stringify(report.device_used ?? null)}, expected ${JSON.stringify(expectDevice)}`,
    );
  }
  return failures;
}

const USAGE =
  "usage: node scripts/ci/assert-selftest.mjs <selftest.json> --exit-code <n> [--expect-device <device>]";

function parseArgs(argv) {
  let file;
  let exitCode;
  let expectDevice;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--exit-code" || a === "--expect-device") {
      const v = argv[++i];
      if (v === undefined) return null;
      if (a === "--exit-code") {
        if (!/^-?\d+$/.test(v)) return null;
        exitCode = Number(v);
      } else {
        expectDevice = v;
      }
    } else if (file === undefined) {
      file = a;
    } else {
      return null;
    }
  }
  if (file === undefined || exitCode === undefined) return null;
  return { file, exitCode, expectDevice };
}

function main(argv) {
  const args = parseArgs(argv);
  if (!args) {
    console.error(USAGE);
    return 2;
  }
  let text;
  try {
    text = readFileSync(args.file, "utf8");
  } catch (e) {
    console.error(`FAIL: cannot read ${args.file}: ${e.message} (exit code was ${args.exitCode})`);
    return 1;
  }
  let report;
  try {
    report = JSON.parse(text.replace(/^﻿/, ""));
  } catch (e) {
    console.error(`FAIL: ${args.file} is not valid JSON: ${e.message}`);
    return 1;
  }
  for (const s of Array.isArray(report?.steps) ? report.steps : []) {
    console.log(`${s.ok ? "ok  " : "FAIL"} ${s.name} (${s.millis} ms): ${s.detail}`);
  }
  const failures = checkSelfTest(report, args);
  if (failures.length) {
    for (const f of failures) console.error(`FAIL: ${f}`);
    return 1;
  }
  console.log(`self-test passed: ${STEP_NAMES.length} steps, device ${report.device_used}`);
  return 0;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
