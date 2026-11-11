# Shared settings for the README screenshots, sourced by the numbered tapes.
# Run scripts/screenshots.sh to regenerate them; see https://github.com/charmbracelet/vhs for the syntax.

# VHS always records a video too: keep it out of the repository
Output "/tmp/vhs.gif"

Set Shell bash
Set Theme "Ubuntu"
Set FontSize 20
Set Width 1920
Set Height 1000
Set Padding 10

# the terminal used by VHS claims sixel support but cannot draw images: use colored half blocks
Env LANG "en_US.UTF-8"
Env MDEDIT_HALFBLOCKS "1"

Hide
Type "clear; target/x86_64-unknown-linux-musl/release/mdedit test-docs/TEST.md"
Enter
Sleep 3s
