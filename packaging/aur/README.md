# AUR packages

| Package | What it does |
|---|---|
| `subforge` | Builds from the tagged GitHub source (needs `cargo`, `nodejs`, `npm`). |
| `subforge-bin` | Repackages the `.deb` from the GitHub release (fast install). |

## Before submitting

- Review the [Arch Rust package guidelines](https://wiki.archlinux.org/title/Rust_package_guidelines)
  (consider `RUSTUP_TOOLCHAIN=stable` and a `--frozen` build after `prepare()`).
- Run `namcap PKGBUILD` and `namcap` on the built package.
- Build in a clean chroot (`extra-x86_64-build`).

## Checksums

Neither package may be submitted with `sha256sums=('SKIP')`. Both carry the real
sum for the current version: `subforge-bin` hashes the `.deb` from the release,
`subforge` hashes the tag archive. The release also publishes `SHA256SUMS.txt`
over the installers (`scripts/release-checksums.sh`), which is the quickest way to
find the `.deb` sum.

## Publishing (maintainer steps)

1. Push the git tag `vX.Y.Z` and wait for the Release workflow to publish the assets.
2. In each package directory: bump `pkgver`, reset `pkgrel=1`, then `updpkgsums`.
3. Test: `makepkg -sf` (and `namcap PKGBUILD *.pkg.tar.zst` if installed).
4. `makepkg --printsrcinfo > .SRCINFO`
5. Push `PKGBUILD` and `.SRCINFO` to `ssh://aur@aur.archlinux.org/<pkgname>.git`.

SubForge checks for MKVToolNix and FFmpeg on startup, so a missing
package never leaves users guessing.
