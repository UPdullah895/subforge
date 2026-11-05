#!/usr/bin/env node
// Fails when a built binary contains AVX-512 instructions in ggml/whisper code.
//
// ggml is compiled with `-march=native` by default, so a release built on an AVX-512
// runner ships AVX-512 code and dies with SIGILL (`Illegal instruction`) on every CPU
// without it — silently, because CI runs on AVX-512 runners too. v0.3.0 shipped like that.
// `packaging/ci/ggml-portable.cmake` turns native off; this check proves it stayed off.
//
// usage: node scripts/ci/check-no-avx512.mjs <binary> [more binaries…]

import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { existsSync } from "node:fs";

const run = promisify(execFile);

/** Counts AVX-512 (zmm) instructions inside symbols whose name mentions ggml or whisper. */
async function avx512Instructions(binary) {
  // `objdump -d` on a 60 MB binary is a few hundred MB of text: parse it as a stream.
  const child = execFile("objdump", ["-d", "--no-show-raw-insn", binary], {
    maxBuffer: 1024 * 1024 * 1024,
  });
  let current = "";
  let ggml = 0;
  let other = 0;
  let pending = "";
  child.stdout.setEncoding("utf8");
  child.stdout.on("data", (chunk) => {
    const text = pending + chunk;
    const lines = text.split("\n");
    pending = lines.pop() ?? "";
    for (const line of lines) {
      const symbol = /^[0-9a-f]+ <(.+)>:$/.exec(line);
      if (symbol) {
        current = symbol[1];
        continue;
      }
      if (!line.includes("zmm")) continue;
      if (/ggml|whisper/i.test(current)) ggml += 1;
      else other += 1;
    }
  });
  await new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", (code) => (code === 0 ? resolve() : reject(new Error(`objdump exited ${code}`))));
  });
  return { ggml, other };
}

const binaries = process.argv.slice(2).filter((p) => !p.startsWith("--"));
if (!binaries.length) {
  console.error("usage: node scripts/ci/check-no-avx512.mjs <binary> [more binaries…]");
  process.exit(2);
}

let failed = false;
for (const binary of binaries) {
  if (!existsSync(binary)) {
    console.error(`::error::${binary} does not exist`);
    failed = true;
    continue;
  }
  const { ggml, other } = await avx512Instructions(binary);
  const name = binary.split("/").pop();
  if (ggml > 0) {
    console.error(
      `::error::${name}: ${ggml} AVX-512 instruction(s) in ggml/whisper code — this build ` +
        `crashes with SIGILL on CPUs without AVX-512. Check that GGML_NATIVE is off ` +
        `(packaging/ci/ggml-portable.cmake) and that the CMake cache was not reused.`
    );
    failed = true;
  } else {
    // Rust's std/libm do runtime-dispatch AVX-512 of their own; that is safe and expected.
    console.log(`${name}: no AVX-512 in ggml/whisper code (${other} elsewhere, runtime-dispatched)`);
  }
}

process.exit(failed ? 1 : 0);
