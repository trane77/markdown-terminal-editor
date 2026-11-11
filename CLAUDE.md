# mdedit

A Markdown editor for the terminal, written in Rust (ratatui + crossterm): editor and live preview,
side by side or stacked, with scroll sync. The binary is `mdedit`. Runs on Linux, Windows and macOS.

## Commands

```bash
cargo test
cargo clippy --all-targets
cargo fmt                      # max_width = 120 (rustfmt.toml)
scripts/screenshots.sh         # regenerate docs/*.png from docs/tapes/ (needs Docker)
scripts/release.sh X.Y.Z       # release, see below
```

To check that platform-specific code compiles: `cargo check --target x86_64-pc-windows-gnu` and
`cargo check --target aarch64-apple-darwin` (add the targets with `rustup target add`).

## Layout

| File | Contents |
|---|---|
| `src/main.rs` | App state, startup, saving |
| `src/input.rs` | Keyboard and mouse handling, links, clipboard |
| `src/ui.rs` | Drawing the panes, scrollbars, status bar and dialogs |
| `src/markdown.rs` | Markdown-to-preview rendering, word wrap, syntax highlighting |
| `src/images.rs` | Image sizing, encoding and caches |
| `src/i18n.rs` | Interface translations and language detection |

## Conventions

- Code, comments, doc comments and string literals (test data included) are in English. The only
  non-English text is in the translation tables in `src/i18n.rs`.
- Every user-visible string goes through `tr()`, never a literal in the UI code.
- Line endings of the edited file are preserved; control characters in documents are neutralized in
  the preview. Don't break either.

## Languages (`src/i18n.rs`)

Six interface languages: English (default and fallback), Italian, Spanish, German, Portuguese,
French. Each one is a `const` of type `Tr`, a struct with one `&'static str` field per UI string.

**Detection** happens once, in `tr()` (cached in a `OnceLock`):

1. The POSIX variables `LC_ALL` > `LC_MESSAGES` > `LANG`, first non-empty one. They win on every
   OS, so `LANG=de_DE.UTF-8 mdedit` forces German on Windows and macOS too.
2. If none is set (usual on Windows, and in macOS terminals that don't export `LANG`), the OS
   preferred UI language via the `sys-locale` crate: `GetUserPreferredUILanguages` on Windows,
   the preferred languages on macOS. These come as BCP 47 (`it-IT`).
3. `lang()` maps the first two letters of the locale (`it_IT.UTF-8` and `it-IT` alike) to a table;
   anything else (`C`, `POSIX`, empty, untranslated) is English.

**Adding a string:** add the field to `Tr` and a value to *every* table (the compiler enforces it),
then use `tr().field`. Values are plain text; dynamic parts are added with `format!` at the call
site (e.g. `format!("{}: {e}", tr().save_error)`), so keep the translated text placeholder-free.
`hints` is an array of the shortcut labels in a fixed key order (see its doc comment).

**Adding a language:** add a `const XX: Tr`, a `Some("xx") => &XX` arm in `lang()`, an assertion
in the `picks_language_from_locale` test, and update the language list in `README.md`.

## Releases

`scripts/release.sh X.Y.Z` (run on a clean tree, on the branch to release, usually `main`): sets the
version in `Cargo.toml`/`Cargo.lock`, runs the tests, regenerates the screenshots, commits (skipped
if nothing changed), tags `vX.Y.Z` and pushes. The tag triggers `.github/workflows/release.yml`,
which builds Linux (musl), Windows and macOS (arm64, x86_64) binaries and publishes a GitHub
release with them, `LICENSE` and a `README.md` whose relative links are pinned to the tag.

License: GPL-3.0-only.
