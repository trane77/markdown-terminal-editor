#!/usr/bin/env bash
# Cuts a release: sets the version in Cargo.toml, regenerates the README screenshots, commits,
# tags vX.Y.Z and pushes. Needs Docker and rustup (see scripts/screenshots.sh).
# The pushed tag triggers .github/workflows/release.yml, which builds the Linux, Windows
# and macOS binaries and attaches them to a GitHub release.
#
#   scripts/release.sh 0.2.0
set -euo pipefail
cd "$(dirname "$0")/.."

version=${1:?usage: scripts/release.sh X.Y.Z}
tag="v$version"

[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must be X.Y.Z, got '$version'" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "working tree is not clean" >&2; exit 1; }
! git rev-parse -q --verify "refs/tags/$tag" >/dev/null || { echo "tag $tag already exists" >&2; exit 1; }

# Only the [package] version line: it is the first top-level "version =" in Cargo.toml.
sed -i "0,/^version = \".*\"/s//version = \"$version\"/" Cargo.toml
cargo update --workspace --offline   # refresh mdedit's version in Cargo.lock only
cargo test --locked
scripts/screenshots.sh   # the release README links to the screenshots at this tag
git add Cargo.toml Cargo.lock docs

# Nothing to commit when the version is already X.Y.Z and the screenshots did not change:
# then the current commit is tagged as it is.
git diff --cached --quiet || git commit -m "Release $tag"
git tag -a "$tag" -m "$tag"
git push origin HEAD "$tag"
