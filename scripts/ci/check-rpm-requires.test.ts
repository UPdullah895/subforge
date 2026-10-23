// @vitest-environment node
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { configuredDepends, missingRequires } from "./check-rpm-requires.mjs";

const script = join(__dirname, "check-rpm-requires.mjs");

// Real `rpm -qpR` output shape: one requirement per line, rich dependencies in parentheses.
const RPM_OUTPUT = `/bin/sh
(ffmpeg or ffmpeg-free)
(vulkan-loader or libvulkan1)
libc.so.6()(64bit)
mkvtoolnix
rpmlib(CompressedFileNames) <= 3.0.4-1
`;

describe("missingRequires", () => {
  it("finds every configured dependency, rich ones included", () => {
    expect(
      missingRequires(RPM_OUTPUT, ["mkvtoolnix", "(ffmpeg or ffmpeg-free)", "(vulkan-loader or libvulkan1)"]),
    ).toEqual([]);
  });

  it("reports a rich dependency that was flattened or lost", () => {
    const flattened = RPM_OUTPUT.replace("(ffmpeg or ffmpeg-free)", "ffmpeg");
    expect(missingRequires(flattened, ["(ffmpeg or ffmpeg-free)", "mkvtoolnix"])).toEqual([
      "(ffmpeg or ffmpeg-free)",
    ]);
  });

  it("matches whole lines, not substrings", () => {
    expect(missingRequires("mkvtoolnix-gui\n", ["mkvtoolnix"])).toEqual(["mkvtoolnix"]);
  });
});

describe("configuredDepends", () => {
  it("reads bundle.linux.rpm.depends from tauri.conf.json and includes the ffmpeg rich dep", () => {
    expect(configuredDepends()).toContain("(ffmpeg or ffmpeg-free)");
  });
});

describe("CLI", () => {
  // A fake `rpm` on PATH, so the CLI can be tested without rpm installed.
  function fakeRpm(output: string) {
    const dir = mkdtempSync(join(tmpdir(), "fake-rpm-"));
    const bin = join(dir, "rpm");
    writeFileSync(bin, `#!/bin/sh\ncat <<'EOF'\n${output}EOF\n`);
    chmodSync(bin, 0o755);
    return { ...process.env, PATH: `${dir}:${process.env.PATH}` };
  }

  it.skipIf(process.platform === "win32")("exits 0 when the rpm declares every dependency", () => {
    const res = spawnSync(process.execPath, [script, "x.rpm"], {
      encoding: "utf8",
      env: fakeRpm(RPM_OUTPUT),
    });
    expect(res.stderr).toBe("");
    expect(res.status).toBe(0);
    expect(res.stdout).toContain("(ffmpeg or ffmpeg-free)");
  });

  it.skipIf(process.platform === "win32")("exits 1 and names the missing dependency", () => {
    const res = spawnSync(process.execPath, [script, "x.rpm"], {
      encoding: "utf8",
      env: fakeRpm(RPM_OUTPUT.replace("(ffmpeg or ffmpeg-free)\n", "")),
    });
    expect(res.status).toBe(1);
    expect(res.stderr).toContain("(ffmpeg or ffmpeg-free)");
  });

  it("exits 2 without an rpm argument", () => {
    expect(spawnSync(process.execPath, [script], { encoding: "utf8" }).status).toBe(2);
  });
});
