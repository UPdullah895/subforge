#!/usr/bin/env node
// Downloads the tools that ship inside the Windows installer and the AppImage (FFmpeg,
// MKVToolNix, the Vulkan loader), verifies their sha256 against packaging/bundled-tools.json and
// lays them out under src-tauri/binaries/: bin/ and licenses/ (both platforms), beside-exe/
// (Windows: vulkan-1.dll, installed next to subforge.exe), lib/ (AppImage: libvulkan.so.1) and
// mkvtoolnix/ (AppImage: the MKVToolNix binaries and libraries behind the bin/ launchers).
// Tauri picks them up through tauri.windows.conf.json / tauri.linux.conf.json; the deb/rpm
// packages never include them.
//
//   node scripts/fetch-bundled-tools.mjs <windows-x64|linux-x64>
//   node scripts/fetch-bundled-tools.mjs --verify-only        # check the manifest only
//   node scripts/fetch-bundled-tools.mjs --sources <dir>      # GPL/LGPL corresponding sources
//   options: --manifest <file>   (default packaging/bundled-tools.json)
//
// Downloads are cached in $SUBFORGE_TOOLS_CACHE (default src-tauri/binaries/.cache) and reused
// when their checksum matches; a second run with an unchanged manifest does nothing.
// Needs: tar (all platforms), 7z for .7z/.zip archives (Windows runners ship 7z.exe), and Linux
// for the AppImage extraction. Only Node's built-in modules are used.

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  copyFileSync,
  createReadStream,
  createWriteStream,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import http from "node:http";
import https from "node:https";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const OUT = path.join(ROOT, "src-tauri", "binaries");
const DEFAULT_MANIFEST = path.join(ROOT, "packaging", "bundled-tools.json");
const PLATFORMS = ["windows-x64", "linux-x64"];
const ARCHIVES = ["zip", "7z", "tar", "tar.xz", "appimage", "deb"];
// Archives a licence file can be taken from (`extra_licenses[].archive`).
const LICENCE_ARCHIVES = ["zip", "7z", "tar"];
const TOOLS = ["ffmpeg", "ffprobe", "mkvmerge", "mkvextract", "mkvpropedit"];
// Bump to force a re-layout when this script changes how it writes the output.
const LAYOUT_VERSION = 1;

class UserError extends Error {}

// ---------------------------------------------------------------- manifest validation

const isSha256 = (s) => typeof s === "string" && /^[0-9a-f]{64}$/.test(s);
/** https only; plain http is accepted for loopback hosts (the tests' local server). */
const isUrl = (s) =>
  typeof s === "string" &&
  (/^https:\/\/\S+$/.test(s) || /^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?\/\S*$/.test(s));
const isPlatformKey = (k) => !k.startsWith("$") && k !== "sources";

/** A relative path that stays inside its base folder. */
function isSafeRelative(p) {
  if (typeof p !== "string" || p === "" || path.isAbsolute(p) || /^[a-zA-Z]:/.test(p)) return false;
  return !p.split(/[\\/]/).some((part) => part === ".." || part === "");
}

/** Returns a list of problems; empty when the manifest is usable. */
export function validateManifest(manifest) {
  const errors = [];
  const platforms = Object.keys(manifest).filter(isPlatformKey);
  for (const p of PLATFORMS) if (!platforms.includes(p)) errors.push(`${p}: platform missing`);
  for (const platform of platforms) {
    if (!PLATFORMS.includes(platform)) {
      errors.push(`${platform}: unknown platform (expected ${PLATFORMS.join(", ")})`);
      continue;
    }
    const exe = platform.startsWith("windows") ? ".exe" : "";
    const provided = new Set();
    for (const [group, e] of Object.entries(manifest[platform])) {
      const at = `${platform}.${group}`;
      if (typeof e.version !== "string" || !e.version) errors.push(`${at}: version missing`);
      if (!isUrl(e.url)) errors.push(`${at}: url missing or not https`);
      if (!isSha256(e.sha256)) errors.push(`${at}: sha256 missing or not 64 lowercase hex digits`);
      if (!ARCHIVES.includes(e.archive)) errors.push(`${at}: archive must be one of ${ARCHIVES.join(", ")}`);
      if (!e.files || typeof e.files !== "object" || !Object.keys(e.files).length) {
        errors.push(`${at}: files missing`);
      }
      for (const [from, to] of Object.entries(e.files ?? {})) {
        if (!isSafeRelative(from)) errors.push(`${at}: bad path in archive ${JSON.stringify(from)}`);
        if (!isSafeRelative(to)) errors.push(`${at}: bad destination ${JSON.stringify(to)}`);
        provided.add(to);
      }
      for (const [to, target] of Object.entries(e.wrappers ?? {})) {
        if (!isSafeRelative(to) || !isSafeRelative(target)) {
          errors.push(`${at}: bad wrapper ${JSON.stringify(to)} -> ${JSON.stringify(target)}`);
        }
        provided.add(to);
      }
      if (!Array.isArray(e.license_files)) errors.push(`${at}: license_files must be a list`);
      for (const f of e.license_files ?? []) {
        if (!isSafeRelative(f)) errors.push(`${at}: bad licence path ${JSON.stringify(f)}`);
      }
      (e.extra_licenses ?? []).forEach((l, i) => {
        const li = `${at}.extra_licenses[${i}]`;
        if (!isSafeRelative(l.name)) errors.push(`${li}: bad name`);
        if (!isUrl(l.url)) errors.push(`${li}: url missing or not https`);
        if (!isSha256(l.sha256)) errors.push(`${li}: sha256 missing or not 64 lowercase hex digits`);
        if (l.archive !== undefined) {
          if (!LICENCE_ARCHIVES.includes(l.archive)) {
            errors.push(`${li}: archive must be one of ${LICENCE_ARCHIVES.join(", ")}`);
          }
          if (!isSafeRelative(l.path)) errors.push(`${li}: path inside the archive missing or unsafe`);
        } else if (l.path !== undefined) {
          errors.push(`${li}: path needs an archive`);
        }
      });
      if (!(e.license_files ?? []).length && !(e.extra_licenses ?? []).length) {
        errors.push(`${at}: no licence files`);
      }
    }
    for (const tool of TOOLS) {
      if (!provided.has(`bin/${tool}${exe}`)) errors.push(`${platform}: nothing provides bin/${tool}${exe}`);
    }
  }
  errors.push(...validateSources(manifest.sources));
  return errors;
}

/** `sources`: the corresponding source archives of the GPL/LGPL code we redistribute. */
function validateSources(sources) {
  if (!Array.isArray(sources) || !sources.length) return ["sources: missing or empty list"];
  const errors = [];
  const files = new Set();
  sources.forEach((s, i) => {
    const at = `sources[${i}]`;
    if (typeof s.name !== "string" || !s.name) errors.push(`${at}: name missing`);
    if (typeof s.version !== "string" || !s.version) errors.push(`${at}: version missing`);
    if (!isUrl(s.url)) errors.push(`${at}: url missing or not https`);
    if (!isSha256(s.sha256)) errors.push(`${at}: sha256 missing or not 64 lowercase hex digits`);
    if (!isSafeRelative(s.file) || /[\\/]/.test(s.file)) errors.push(`${at}: file must be a plain file name`);
    else if (files.has(s.file)) errors.push(`${at}: duplicate file ${s.file}`);
    else files.add(s.file);
    const badPlatforms =
      s.platforms !== undefined &&
      (!Array.isArray(s.platforms) || s.platforms.some((p) => !PLATFORMS.includes(p)));
    if (badPlatforms) errors.push(`${at}: platforms must list known platforms`);
  });
  return errors;
}

// ---------------------------------------------------------------- download and verify

function sha256File(file) {
  return new Promise((resolve, reject) => {
    const h = createHash("sha256");
    createReadStream(file)
      .on("data", (d) => h.update(d))
      .on("end", () => resolve(h.digest("hex")))
      .on("error", reject);
  });
}

function download(url, dest, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > 10) return reject(new Error(`too many redirects for ${url}`));
    const mod = url.startsWith("https:") ? https : http;
    const req = mod.get(url, { headers: { "User-Agent": "subforge-fetch-bundled-tools" } }, (res) => {
      if ([301, 302, 303, 307, 308].includes(res.statusCode) && res.headers.location) {
        res.resume();
        return resolve(download(new URL(res.headers.location, url).toString(), dest, redirects + 1));
      }
      if (res.statusCode !== 200) {
        res.resume();
        return reject(new UserError(`download of ${url} failed: HTTP ${res.statusCode}`));
      }
      const out = createWriteStream(dest);
      res.pipe(out);
      out.on("finish", () => out.close(resolve));
      out.on("error", reject);
      res.on("error", reject);
    });
    req.setTimeout(120_000, () => req.destroy(new Error(`download of ${url} timed out`)));
    req.on("error", reject);
  });
}

/** Cached, checksum-verified copy of `url`. A cached file with the wrong checksum is replaced. */
async function fetchVerified(cache, url, sha256) {
  mkdirSync(cache, { recursive: true });
  const name = `${sha256.slice(0, 12)}-${path.basename(new URL(url).pathname)}`;
  const file = path.join(cache, name);
  if (existsSync(file) && (await sha256File(file)) === sha256) return file;
  console.log(`  downloading ${url}`);
  const part = `${file}.part`;
  rmSync(part, { force: true });
  try {
    await download(url, part);
  } catch (e) {
    rmSync(part, { force: true });
    throw e;
  }
  const actual = await sha256File(part);
  if (actual !== sha256) {
    rmSync(part, { force: true });
    throw new UserError(`checksum mismatch for ${url}\n  expected ${sha256}\n  actual   ${actual}`);
  }
  renameSync(part, file);
  return file;
}

// ---------------------------------------------------------------- extraction

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { stdio: ["ignore", "ignore", "pipe"], encoding: "utf8", ...opts });
  if (r.error) throw new UserError(`could not run ${cmd}: ${r.error.message}`);
  if (r.status !== 0) throw new UserError(`${cmd} ${args.join(" ")} failed (${r.status}): ${r.stderr}`);
}

function find7z() {
  const candidates =
    process.platform === "win32"
      ? ["7z", path.join(process.env.ProgramFiles ?? "C:\\Program Files", "7-Zip", "7z.exe")]
      : ["7z", "7za", "7zz"];
  for (const c of candidates) {
    if (!spawnSync(c, ["i"], { stdio: "ignore" }).error) return c;
  }
  throw new UserError(
    `7-Zip is needed to unpack .7z/.zip archives but none of ${candidates.join(", ")} was found. ` +
      "Install p7zip (Linux) or 7-Zip (Windows) and try again.",
  );
}

/** Reads an `ar` archive (a .deb) and returns its members as {name, data}. */
function readAr(file) {
  const buf = readFileSync(file);
  if (buf.subarray(0, 8).toString("latin1") !== "!<arch>\n") throw new UserError(`${file} is not a .deb`);
  const members = [];
  let off = 8;
  while (off + 60 <= buf.length) {
    const header = buf.subarray(off, off + 60).toString("latin1");
    const name = header.slice(0, 16).trim().replace(/\/$/, "");
    const size = parseInt(header.slice(48, 58).trim(), 10);
    off += 60;
    members.push({ name, data: buf.subarray(off, off + size) });
    off += size + (size % 2);
  }
  return members;
}

/** Unpacks `archive` once into a folder keyed by its checksum; returns the folder to read from. */
function extract(entry, archive, cache) {
  const dir = path.join(cache, "x", entry.sha256.slice(0, 16));
  const root = entry.archive === "appimage" ? path.join(dir, "squashfs-root") : dir;
  if (existsSync(path.join(dir, ".done"))) return root;
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  switch (entry.archive) {
    case "zip":
    case "7z":
      run(find7z(), ["x", "-y", `-o${dir}`, archive]);
      break;
    case "tar":
    case "tar.xz":
      run("tar", ["-xf", archive, "-C", dir]);
      break;
    case "deb": {
      const data = readAr(archive).find((m) => m.name.startsWith("data.tar"));
      if (!data) throw new UserError(`${archive} has no data.tar member`);
      const tarball = path.join(dir, data.name);
      writeFileSync(tarball, data.data);
      run("tar", ["-xf", tarball, "-C", dir]);
      rmSync(tarball);
      break;
    }
    case "appimage":
      if (process.platform !== "linux") throw new UserError("AppImages can only be unpacked on Linux");
      chmodSync(archive, 0o755);
      run(archive, ["--appimage-extract"], { cwd: dir });
      break;
  }
  writeFileSync(path.join(dir, ".done"), "");
  return root;
}

// ---------------------------------------------------------------- layout

function copyInto(from, to, executable) {
  if (!existsSync(from)) throw new UserError(`expected ${from} in the archive, but it is missing`);
  mkdirSync(path.dirname(to), { recursive: true });
  copyFileSync(from, to);
  if (executable && process.platform !== "win32") chmodSync(to, 0o755);
}

/**
 * Shell launcher in bin/ that runs the real binary with its private libraries. `target` is
 * relative to src-tauri/binaries/. In the AppImage, bin/ is installed as usr/lib/subforge/bin
 * and the payload as usr/share/subforge/<target> (tauri.linux.conf.json): it must stay out of
 * usr/lib, where linuxdeploy would try to resolve the payload's libraries and fail.
 */
function wrapperScript(target) {
  return `#!/bin/sh
# Generated by scripts/fetch-bundled-tools.mjs: runs the bundled ${path.basename(target)} with its libraries.
here=$(dirname "$(readlink -f "$0")")
real="$here/../${target}"
[ -x "$real" ] || real="$here/../../../share/subforge/${target}"
LD_LIBRARY_PATH="$(dirname "$real")/lib\${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" exec "$real" "$@"
`;
}

function clearOutput() {
  if (!existsSync(OUT)) return;
  for (const name of readdirSync(OUT)) {
    if (name === ".cache" || name === ".gitignore") continue;
    rmSync(path.join(OUT, name), { recursive: true, force: true });
  }
}

async function fetchPlatform(manifest, platform) {
  const entries = manifest[platform];
  const cache = path.resolve(process.env.SUBFORGE_TOOLS_CACHE || path.join(OUT, ".cache"));
  const stampFile = path.join(OUT, ".stamp.json");
  const hash = createHash("sha256")
    .update(JSON.stringify({ LAYOUT_VERSION, platform, entries }))
    .digest("hex");
  const outputs = Object.values(entries).flatMap((e) => [
    ...Object.values(e.files),
    ...Object.keys(e.wrappers ?? {}),
  ]);
  if (existsSync(stampFile)) {
    const stamp = JSON.parse(readFileSync(stampFile, "utf8"));
    if (stamp.hash === hash && outputs.every((o) => existsSync(path.join(OUT, o)))) {
      console.log(`Bundled tools for ${platform} are up to date in ${OUT}`);
      return;
    }
  }

  clearOutput();
  for (const [group, e] of Object.entries(entries)) {
    console.log(`${group} ${e.version}`);
    const archive = await fetchVerified(cache, e.url, e.sha256);
    const root = extract(e, archive, cache);
    for (const [from, to] of Object.entries(e.files)) {
      copyInto(path.join(root, from), path.join(OUT, to), true);
    }
    for (const [to, target] of Object.entries(e.wrappers ?? {})) {
      const file = path.join(OUT, to);
      mkdirSync(path.dirname(file), { recursive: true });
      writeFileSync(file, wrapperScript(target));
      chmodSync(file, 0o755);
    }
    const licenses = path.join(OUT, "licenses", group);
    for (const f of e.license_files) copyInto(path.join(root, f), path.join(licenses, path.basename(f)), false);
    for (const l of e.extra_licenses ?? []) {
      const file = await fetchVerified(cache, l.url, l.sha256);
      const from = l.archive ? path.join(extract(l, file, cache), l.path) : file;
      copyInto(from, path.join(licenses, l.name), false);
    }
    writeFileSync(
      path.join(licenses, "SOURCE.txt"),
      `${group} ${e.version}\nDownloaded from: ${e.url}\nsha256: ${e.sha256}\nProject: ${e.source ?? ""}\n`,
    );
  }
  copyInto(path.join(ROOT, "THIRD_PARTY_NOTICES.md"), path.join(OUT, "licenses", "THIRD_PARTY_NOTICES.md"), false);
  writeFileSync(stampFile, JSON.stringify({ platform, hash }, null, 2) + "\n");
  console.log(`Bundled tools for ${platform} are in ${OUT}`);
}

// ---------------------------------------------------------------- CLI

/**
 * Downloads every `sources` archive into `dir`, checksum-verified. Fails on the first problem;
 * a file only appears in `dir` once it has been verified.
 */
async function fetchSources(manifest, dir) {
  const cache = path.resolve(process.env.SUBFORGE_TOOLS_CACHE || path.join(OUT, ".cache"));
  mkdirSync(dir, { recursive: true });
  for (const s of manifest.sources) {
    console.log(`${s.name} ${s.version}`);
    const file = await fetchVerified(cache, s.url, s.sha256);
    const dest = path.join(dir, s.file);
    copyFileSync(file, `${dest}.part`);
    renameSync(`${dest}.part`, dest);
  }
  console.log(`${manifest.sources.length} source archives verified in ${dir}`);
}

async function main(argv) {
  let manifestPath = DEFAULT_MANIFEST;
  let verifyOnly = false;
  let sourcesDir;
  let platform;
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--verify-only") verifyOnly = true;
    else if (a === "--manifest") manifestPath = path.resolve(argv[++i] ?? "");
    else if (a === "--sources") {
      if (!argv[i + 1]) throw new UserError("--sources needs a folder");
      sourcesDir = path.resolve(argv[++i]);
    } else if (!platform) platform = a;
    else throw new UserError(`unexpected argument ${a}`);
  }
  if (!verifyOnly && !sourcesDir && !PLATFORMS.includes(platform)) {
    throw new UserError(
      `unknown platform ${JSON.stringify(platform ?? "")}; usage: fetch-bundled-tools.mjs <${PLATFORMS.join("|")}> | --verify-only | --sources <dir>`,
    );
  }
  const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
  const errors = validateManifest(manifest);
  if (errors.length) throw new UserError(`${manifestPath} is invalid:\n  ${errors.join("\n  ")}`);
  if (verifyOnly) {
    for (const p of PLATFORMS) {
      console.log(`${p}: ${Object.entries(manifest[p]).map(([g, e]) => `${g} ${e.version}`).join(", ")}`);
    }
    const names = manifest.sources.map((s) => `${s.name} ${s.version}`).join(", ");
    console.log(`sources: ${manifest.sources.length} archives (${names})`);
    console.log("Manifest OK: every entry has a version, url and sha256.");
    return;
  }
  if (sourcesDir) {
    await fetchSources(manifest, sourcesDir);
    return;
  }
  await fetchPlatform(manifest, platform);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch((e) => {
    console.error(e instanceof UserError ? `error: ${e.message}` : e);
    process.exit(1);
  });
}
