# Contributing to SubForge

Thanks for helping! Bug reports, translations and pull requests are all welcome.

## Setup
```bash
git clone https://github.com/UPdullah895/subforge.git && cd subforge
npm install
npm run tauri dev
```
You also need MKVToolNix, FFmpeg and Python (see the README); SubForge will tell you what is missing.

## Before opening a PR
```bash
npm run check:versions && npm run lint && npx vue-tsc --noEmit && npm test
(cd crates/subforge-deps && cargo test --features tokio)
(cd crates/subforge-metadata && cargo test)
(cd crates/subforge-subtitle-sources && cargo test)
(cd src-tauri && cargo test)
(cd server && cargo test)
```
Set `SUBFORGE_SUBSOURCE_API_KEY=ci-placeholder-key` for the Tauri/server builds if you have no key.

## Guidelines
- Keep changes focused; add or update tests for behaviour changes.
- User-visible text goes in `src/i18n/en.json` **and** `ar.json`.
- The desktop app and `server/` share the same frontend; new backend features need both a Tauri command and a REST route.
- Never build shell command strings from user input — pass argument arrays.

## Releasing (maintainers)
1. Bump the version in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `server/Cargo.toml` (`npm run check:versions` verifies).
2. Update `CHANGELOG.md`, commit, tag `vX.Y.Z`, push the tag — the Release workflow builds Windows and Linux installers.
3. Update the AUR packages (see `packaging/aur/README.md`).
