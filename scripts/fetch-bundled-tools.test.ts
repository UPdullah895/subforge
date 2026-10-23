// @vitest-environment node
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";

const script = join(__dirname, "fetch-bundled-tools.mjs");
const manifestPath = join(__dirname, "..", "packaging", "bundled-tools.json");

function run(...args: string[]) {
  return spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
}

type Entry = {
  sha256?: string;
  files: Record<string, string>;
  extra_licenses?: { sha256: string }[];
};
type Manifest = Record<string, Record<string, Entry>>;

function withManifest(edit: (m: Manifest) => void): string {
  const m = JSON.parse(readFileSync(manifestPath, "utf8"));
  edit(m);
  const p = join(mkdtempSync(join(tmpdir(), "bundled-tools-")), "manifest.json");
  writeFileSync(p, JSON.stringify(m));
  return p;
}

describe("fetch-bundled-tools --verify-only", () => {
  it("accepts the committed manifest", () => {
    const r = run("--verify-only");
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
    expect(r.stdout).toMatch(/windows-x64/);
    expect(r.stdout).toMatch(/linux-x64/);
  });

  it("rejects an entry without a sha256", () => {
    const p = withManifest((m) => {
      delete m["linux-x64"].ffmpeg.sha256;
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/linux-x64\.ffmpeg.*sha256/);
  });

  it("rejects an extra licence without a valid sha256", () => {
    const p = withManifest((m) => {
      m["windows-x64"]["vulkan-loader"].extra_licenses[0].sha256 = "abc";
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/windows-x64\.vulkan-loader\.extra_licenses\[0\].*sha256/);
  });

  it("requires every tool on every platform", () => {
    const p = withManifest((m) => {
      delete m["windows-x64"].mkvtoolnix.files["mkvtoolnix/mkvpropedit.exe"];
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/windows-x64.*mkvpropedit/);
  });

  it("rejects destinations outside the output folder", () => {
    const p = withManifest((m) => {
      m["linux-x64"].ffmpeg.files["x/ffmpeg"] = "../escape";
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/\.\.\/escape/);
  });

  it("rejects an unknown platform", () => {
    const r = run("macos-arm64");
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/macos-arm64/);
  });
});
