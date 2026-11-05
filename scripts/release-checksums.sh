#!/usr/bin/env bash
# Publish SHA256SUMS.txt for a release, so users can verify what they downloaded:
#   sha256sum -c SHA256SUMS.txt --ignore-missing
#
# Hashes the installers *as GitHub serves them* (that is what a user gets), then
# uploads the file next to them. Run by the Release workflow after the builds;
# it can also be run by hand from a maintainer's machine with gh authenticated.
#
# Usage: scripts/release-checksums.sh <tag> [work-dir]
set -euo pipefail

tag=${1:?usage: release-checksums.sh <tag> [work-dir]}
work=${2:-$(mktemp -d)}
mkdir -p "$work"

# Installers only: the bundled GPL/LGPL source archives are not what users install.
gh release download "$tag" --dir "$work" --pattern 'subforge*' --clobber

cd "$work"
ls -1 subforge* >/dev/null 2>&1 || { echo "error: no installers found for $tag" >&2; exit 1; }

# shellcheck disable=SC2012
sha256sum $(ls -1 subforge* | sort) >SHA256SUMS.txt

# Fail loudly if the file does not describe what is in the directory.
sha256sum -c SHA256SUMS.txt --ignore-missing >/dev/null

gh release upload "$tag" SHA256SUMS.txt --clobber
echo "Published SHA256SUMS.txt for $tag:"
cat SHA256SUMS.txt
