# catmd

`catmd` is a `cat`-like CLI that prints files to stdout and adds rich terminal rendering for Markdown files.

## Install (Homebrew)

```bash
brew tap schneidermayer/tap
brew install schneidermayer/tap/catmd
```

## Current behavior

- Prints non-Markdown files exactly like `cat`.
- Renders `*.md`, `*.markdown`, `*.mkd`, and `*.mdown` with ANSI styling when stdout is a TTY.
- `--markdown` forces Markdown rendering for all inputs, including stdin and non-TTY stdout.
- Highlights fenced code blocks (via `syntect`) in rendered Markdown.
- Supports `-` as stdin, and multiple input files in sequence.

## Usage

```bash
catmd README.md
catmd notes.txt README.md
catmd - < README.md
catmd --markdown - < README.md
catmd --plain README.md
```

## Build

Install Rust first (via `rustup`), then:

```bash
cargo build
cargo run -- README.md
cargo test
```

## Distribution

`catmd` is distributed via the `schneidermayer/tap` Homebrew tap.

See [RELEASE.md](RELEASE.md) for release and Homebrew steps.

## Next milestones

- Add richer table and blockquote rendering.
- Add configurable styles/themes for non-code Markdown elements.
- Add integration tests that assert behavior against fixture files.
