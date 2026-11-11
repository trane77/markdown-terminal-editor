#!/usr/bin/env bash
# Regenerates the README screenshots in docs/ from the VHS tapes in docs/tapes/.
#
#   scripts/screenshots.sh                                     # all screenshots
#   scripts/screenshots.sh docs/tapes/screenshot_2_code.tape   # only some of them
#
# Needs Docker and rustup. VHS runs from its official image, so it does not need to be installed;
# mdedit is built as a static (musl) binary so that it runs inside that image.
set -euo pipefail
cd "$(dirname "$0")/.."

vhs_image=ghcr.io/charmbracelet/vhs:v0.10.0
target=x86_64-unknown-linux-musl

tapes=("$@")
if [ ${#tapes[@]} -eq 0 ]; then
    tapes=(docs/tapes/screenshot_*.tape)
fi

rustup target add "$target" >/dev/null
cargo build --release --target "$target"

# run as the current user, so the screenshots are not owned by root
docker run --rm --user "$(id -u):$(id -g)" -e HOME=/tmp -v "$PWD":/repo -w /repo \
    --entrypoint sh "$vhs_image" -c 'for tape in "$@"; do vhs "$tape" || exit 1; done' sh "${tapes[@]}"

echo "Updated: ${tapes[*]}"
