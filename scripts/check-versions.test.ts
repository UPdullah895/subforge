// @vitest-environment node
import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { agreedVersion, collectVersions, lockVersion, pkgbuildVersion } from "./check-versions.mjs";

const LOCK = `[[package]]
name = "subforge-core"
version = "0.1.0"

[[package]]
name = "subforge"
version = "0.3.0"
`;

describe("check-versions", () => {
  it("reads the app package from a Cargo.lock, not a similarly named crate", () => {
    expect(lockVersion(LOCK, "subforge")).toBe("0.3.0");
    expect(lockVersion(LOCK, "subforge-core")).toBe("0.1.0");
    expect(lockVersion(LOCK, "subforge-server")).toBeUndefined();
  });

  it("reads pkgver from a PKGBUILD", () => {
    expect(pkgbuildVersion("pkgname=subforge\npkgver=0.3.0\npkgrel=1\n")).toBe("0.3.0");
  });

  it("covers the lockfiles and PKGBUILDs and flags a mismatch", () => {
    const files: Record<string, string> = {
      "package.json": '{"version":"1.2.3"}',
      "package-lock.json": '{"version":"1.2.3","packages":{"":{"version":"1.2.3"}}}',
      "src-tauri/tauri.conf.json": '{"version":"1.2.3"}',
      "src-tauri/Cargo.toml": '[package]\nname = "subforge"\nversion = "1.2.3"\n',
      "src-tauri/Cargo.lock": '[[package]]\nname = "subforge"\nversion = "1.2.3"\n',
      "server/Cargo.toml": '[package]\nname = "subforge-server"\nversion = "1.2.3"\n',
      "server/Cargo.lock": '[[package]]\nname = "subforge-server"\nversion = "1.2.3"\n',
      "packaging/aur/subforge/PKGBUILD": "pkgver=1.2.3\n",
      "packaging/aur/subforge-bin/PKGBUILD": "pkgver=1.2.3\n",
    };
    const read = (p: string) => files[p];
    expect(agreedVersion(collectVersions(read))).toBe("1.2.3");

    files["server/Cargo.lock"] = '[[package]]\nname = "subforge-server"\nversion = "1.2.2"\n';
    expect(agreedVersion(collectVersions(read))).toBeNull();
    files["server/Cargo.lock"] = '[[package]]\nname = "subforge-server"\nversion = "1.2.3"\n';

    files["packaging/aur/subforge-bin/PKGBUILD"] = "pkgver=1.2.0\n";
    expect(agreedVersion(collectVersions(read))).toBeNull();
  });

  it("the repository's own versions agree", () => {
    const read = (p: string) => readFileSync(join(process.cwd(), p), "utf8");
    expect(agreedVersion(collectVersions(read))).not.toBeNull();
  });
});
