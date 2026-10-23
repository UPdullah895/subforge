// @vitest-environment node
import { spawn, spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const script = join(__dirname, "fetch-bundled-tools.mjs");
const manifestPath = join(__dirname, "..", "packaging", "bundled-tools.json");

function run(...args: string[]) {
  return spawnSync(process.execPath, [script, ...args], { encoding: "utf8" });
}

/** Like `run`, but asynchronous, so a server in this process can answer the child. */
function runAsync(...args: string[]): Promise<{ status: number | null; stdout: string; stderr: string }> {
  return new Promise((resolve) => {
    const cache = mkdtempSync(join(tmpdir(), "tools-cache-"));
    const child = spawn(process.execPath, [script, ...args], {
      env: { ...process.env, SUBFORGE_TOOLS_CACHE: cache },
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("close", (status) => resolve({ status, stdout, stderr }));
  });
}

type Licence = { name?: string; url?: string; sha256: string; archive?: string; path?: string };
type Entry = {
  url?: string;
  sha256?: string;
  files: Record<string, string>;
  extra_licenses?: Licence[];
};
type Source = { name: string; version: string; url: string; sha256?: string; file?: string };
type Manifest = Record<string, Record<string, Entry>> & { sources: Source[] };

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
    expect(r.stdout).toMatch(/sources: \d+/);
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
      m["windows-x64"]["vulkan-loader"].extra_licenses![0].sha256 = "abc";
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/windows-x64\.vulkan-loader\.extra_licenses\[0\].*sha256/);
  });

  it("rejects an archived licence without its path inside the archive", () => {
    const p = withManifest((m) => {
      const l = m["linux-x64"].mkvtoolnix.extra_licenses!.find((x) => x.archive)!;
      delete l.path;
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/linux-x64\.mkvtoolnix\.extra_licenses\[\d+\].*path/);
  });

  it("rejects plain http downloads", () => {
    const p = withManifest((m) => {
      m["linux-x64"]["vulkan-loader"].url = "http://archive.ubuntu.com/x.deb";
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/linux-x64\.vulkan-loader: url .*https/);
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

describe("manifest sources", () => {
  it("requires a sources list", () => {
    const p = withManifest((m) => {
      delete (m as Partial<Manifest>).sources;
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/sources/);
  });

  it("rejects a source without a sha256", () => {
    const p = withManifest((m) => {
      delete m.sources[0].sha256;
    });
    const r = run("--verify-only", "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/sources\[0\].*sha256/);
  });

  it("rejects unsafe or duplicate source file names", () => {
    const bad = withManifest((m) => {
      m.sources[0].file = "../x.tar.gz";
    });
    expect(run("--verify-only", "--manifest", bad).stderr).toMatch(/sources\[0\].*file/);
    const dup = withManifest((m) => {
      m.sources[1].file = m.sources[0].file;
    });
    const r = run("--verify-only", "--manifest", dup);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/sources\[1\].*duplicate/);
  });

  it("covers FFmpeg and MKVToolNix on both platforms", () => {
    const m: Manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    for (const platform of ["linux-x64", "windows-x64"]) {
      for (const name of ["ffmpeg", "mkvtoolnix"]) {
        expect(
          m.sources.some(
            (s) => s.name === name && (s as Source & { platforms: string[] }).platforms.includes(platform),
          ),
          `${name} on ${platform}`,
        ).toBe(true);
      }
    }
  });
});

describe("fetch-bundled-tools --sources", () => {
  const good = Buffer.from("source tarball body");
  const sha = (b: Buffer) => createHash("sha256").update(b).digest("hex");
  let server: Server;
  let base = "";

  beforeAll(async () => {
    server = createServer((req, res) => {
      if (req.url === "/good.tar.gz") res.end(good);
      else if (req.url === "/tampered.tar.gz") res.end(Buffer.from("something else"));
      else {
        res.statusCode = 404;
        res.end();
      }
    });
    await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
    base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  });
  afterAll(() => new Promise<void>((r) => server.close(() => r())));

  const manifestWith = (sources: Source[]) =>
    withManifest((m) => {
      m.sources = sources;
    });

  it("downloads and verifies every source into the folder", async () => {
    const p = manifestWith([
      { name: "demo", version: "1", url: `${base}/good.tar.gz`, sha256: sha(good), file: "demo-1.tar.gz" },
    ]);
    const out = mkdtempSync(join(tmpdir(), "sources-"));
    const r = await runAsync("--sources", out, "--manifest", p);
    expect(r.stderr).toBe("");
    expect(r.status).toBe(0);
    expect(readFileSync(join(out, "demo-1.tar.gz"))).toEqual(good);
  });

  it("fails closed on a checksum mismatch and leaves nothing behind", async () => {
    const p = manifestWith([
      { name: "demo", version: "1", url: `${base}/good.tar.gz`, sha256: sha(good), file: "demo-1.tar.gz" },
      { name: "evil", version: "1", url: `${base}/tampered.tar.gz`, sha256: sha(good), file: "evil-1.tar.gz" },
    ]);
    const out = mkdtempSync(join(tmpdir(), "sources-"));
    const r = await runAsync("--sources", out, "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/checksum mismatch/);
    expect(existsSync(join(out, "evil-1.tar.gz"))).toBe(false);
    expect(readdirSync(out).filter((f) => f.endsWith(".part"))).toEqual([]);
  });

  it("fails on a download error", async () => {
    const p = manifestWith([
      { name: "gone", version: "1", url: `${base}/missing.tar.gz`, sha256: sha(good), file: "gone-1.tar.gz" },
    ]);
    const out = mkdtempSync(join(tmpdir(), "sources-"));
    const r = await runAsync("--sources", out, "--manifest", p);
    expect(r.status).toBe(1);
    expect(r.stderr).toMatch(/HTTP 404/);
    expect(readdirSync(out)).toEqual([]);
  });
});
