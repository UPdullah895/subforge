# Security Policy

## Reporting a vulnerability
Please **do not** open a public issue. Use GitHub's
[private vulnerability reporting](https://github.com/UPdullah895/subforge/security/advisories/new)
on this repository. Expect an acknowledgement within a few days.

## Scope notes
- SubForge runs local tools (`mkvmerge`, `ffmpeg`) and the built-in speech-to-text engine on files you choose.
- Installing dependencies uses a fixed allowlist of packages and never a shell.
- `subforge-server` binds to `127.0.0.1` by default. Do not expose it to untrusted networks;
  software installation through its API is off unless `SUBFORGE_ALLOW_INSTALL=1`.
