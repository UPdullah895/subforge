# Contributing to SubForge

Thanks for helping! Bug reports, translations and pull requests are all welcome.

## Setup
```bash
git clone https://github.com/UPdullah895/subforge.git && cd subforge
npm install
npm run tauri dev
```
You also need MKVToolNix and FFmpeg (see the README); SubForge will tell you what is missing.

## Before opening a PR
```bash
npm run check:versions && npm run lint && npx vue-tsc --noEmit && npm test
(cd crates/subforge-core && cargo test)
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

## In-app update notice

The app asks GitHub for the newest release once a day and offers a link; it never downloads
or installs anything by itself (see `src/stores/updateStore.ts`). Turning that into a real
auto-update needs a Tauri signing keypair and two repository secrets:

1. `npm run tauri signer generate -- -w ~/.tauri/subforge.key` — keep the file out of the
   repo; the command prints the public key.
2. Add `plugins.updater.pubkey` (that public key) and `plugins.updater.endpoints`
   (`https://github.com/UPdullah895/subforge/releases/latest/download/latest.json`) to
   `src-tauri/tauri.conf.json`, set `bundle.createUpdaterArtifacts` to `true`, and add
   `tauri-plugin-updater` to `src-tauri/Cargo.toml` and its init call.
3. Store `TAURI_SIGNING_PRIVATE_KEY` (the key file) and
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as repository secrets — tauri-action then uploads
   the `.sig` files and `latest.json` with every release.

Once `createUpdaterArtifacts` is on, a build without those secrets fails, so land this in a
PR that adds the secrets at the same time.

## Releasing (maintainers)
1. Bump the version in `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `server/Cargo.toml` (`npm run check:versions` verifies).
2. Update `CHANGELOG.md`, commit, tag `vX.Y.Z`, push the tag — the Release workflow builds Windows and Linux installers.
3. Update the AUR packages (see `packaging/aur/README.md`).
