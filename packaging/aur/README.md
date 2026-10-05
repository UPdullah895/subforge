# AUR packages

| Package | What it does |
|---|---|
| `subforge` | Builds from the tagged GitHub source (needs `cargo`, `nodejs`, `npm`). |
| `subforge-bin` | Repackages the `.deb` from the GitHub release (fast install). |

## Publishing (maintainer steps)

1. Push the git tag `vX.Y.Z` and wait for the Release workflow to publish the assets.
2. In each package directory: bump `pkgver`, reset `pkgrel=1`, then `updpkgsums`.
3. Test: `makepkg -sf` (and `namcap PKGBUILD *.pkg.tar.zst` if installed).
4. `makepkg --printsrcinfo > .SRCINFO`
5. Push `PKGBUILD` and `.SRCINFO` to `ssh://aur@aur.archlinux.org/<pkgname>.git`.

SubForge checks for MKVToolNix, FFmpeg, Python and `pysubs2` on startup, so a missing
optional package never leaves users guessing.
