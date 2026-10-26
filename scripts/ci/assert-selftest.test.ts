// @vitest-environment node
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { checkSelfTest, STEP_NAMES } from "./assert-selftest.mjs";

const script = join(__dirname, "assert-selftest.mjs");

function passingReport(device = "cpu") {
  return {
    steps: STEP_NAMES.map((name: string) => ({ name, ok: true, millis: 1, detail: "fine" })),
    ok: true,
    device_used: device,
  };
}

function writeReport(report: unknown): string {
  const dir = mkdtempSync(join(tmpdir(), "selftest-"));
  const file = join(dir, "selftest.json");
  writeFileSync(file, typeof report === "string" ? report : JSON.stringify(report));
  return file;
}

function run(...args: string[]) {
  return spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
}

describe("checkSelfTest", () => {
  it("passes a complete, successful CPU report with exit code 0", () => {
    expect(checkSelfTest(passingReport(), { exitCode: 0, expectDevice: "cpu" })).toEqual([]);
  });

  it("fails on a non-zero exit code", () => {
    expect(checkSelfTest(passingReport(), { exitCode: 1 })).toEqual([
      expect.stringContaining("exit code 1"),
    ]);
  });

  it("fails when the report is not ok and names the failed step with its detail", () => {
    const r = passingReport();
    r.ok = false;
    r.steps[8] = { name: "whisper_tiny", ok: false, millis: 3, detail: "the transcript is empty" };
    const failures = checkSelfTest(r, { exitCode: 1 });
    expect(failures).toContainEqual(expect.stringContaining('"ok" is false'));
    expect(failures).toContainEqual(
      expect.stringMatching(/whisper_tiny failed: the transcript is empty/),
    );
  });

  it("fails when a step is missing (e.g. the run stopped early) even if ok is true", () => {
    const r = passingReport();
    r.steps = r.steps.filter((s: { name: string }) => s.name !== "verify_mux");
    expect(checkSelfTest(r, { exitCode: 0 })).toEqual([
      expect.stringContaining("missing step verify_mux"),
    ]);
  });

  it("fails on an unexpected device", () => {
    const failures = checkSelfTest(passingReport("gpu: NVIDIA GeForce RTX 3070"), {
      exitCode: 0,
      expectDevice: "cpu",
    });
    expect(failures).toEqual([expect.stringContaining('device_used is "gpu: NVIDIA GeForce RTX 3070"')]);
  });

  it("fails when device_used is absent but a device is expected", () => {
    const r: Record<string, unknown> = passingReport();
    delete r.device_used;
    expect(checkSelfTest(r, { exitCode: 0, expectDevice: "cpu" })).toEqual([
      expect.stringContaining("device_used is null"),
    ]);
  });

  it("rejects something that is not a report", () => {
    expect(checkSelfTest({ hello: 1 }, { exitCode: 0 })).toContainEqual(
      expect.stringContaining('no "steps" array'),
    );
  });
});

describe("STEP_NAMES", () => {
  it("matches subforge_core::selftest::STEP_NAMES", () => {
    const src = readFileSync(
      join(__dirname, "..", "..", "crates", "subforge-core", "src", "selftest.rs"),
      "utf8",
    );
    const block = /pub const STEP_NAMES: \[&str; \d+\] = \[([\s\S]*?)\];/.exec(src)?.[1] ?? "";
    const names = [...block.matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    expect(names.length).toBeGreaterThan(0);
    expect(STEP_NAMES).toEqual(names);
  });
});

describe("CLI", () => {
  it("exits 0 and prints a summary for a passing report", () => {
    const res = run(writeReport(passingReport()), "--exit-code", "0", "--expect-device", "cpu");
    expect(res.stderr).toBe("");
    expect(res.status).toBe(0);
    expect(res.stdout).toContain("self-test passed: 11 steps, device cpu");
  });

  it("exits 1 and lists every failure", () => {
    const res = run(writeReport(passingReport("gpu: X")), "--exit-code", "1", "--expect-device", "cpu");
    expect(res.status).toBe(1);
    expect(res.stderr).toContain("exit code 1");
    expect(res.stderr).toContain('device_used is "gpu: X"');
  });

  it("exits 1 when the report file is missing", () => {
    const res = run(join(tmpdir(), "does-not-exist", "selftest.json"), "--exit-code", "0");
    expect(res.status).toBe(1);
    expect(res.stderr).toContain("cannot read");
  });

  it("exits 1 on invalid JSON", () => {
    const res = run(writeReport("{not json"), "--exit-code", "0");
    expect(res.status).toBe(1);
    expect(res.stderr).toContain("not valid JSON");
  });

  it("exits 2 on bad arguments", () => {
    expect(run().status).toBe(2);
    expect(run(writeReport(passingReport()), "--exit-code").status).toBe(2);
    expect(run(writeReport(passingReport()), "--exit-code", "abc").status).toBe(2);
  });
});
