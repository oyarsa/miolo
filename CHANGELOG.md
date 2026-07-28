# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-07-28

### Added

- JSON input: a top-level array of objects, with columns taken from the union
  of every object's keys
- JSONL / NDJSON input, one object per line
- gzip and zstd decompression, detected from the file's leading bytes rather
  than its name, so compressed input works over a pipe and survives a
  misleading filename
- `--format` to state the input format, principally for stdin
- Format inferred from the file extension, including through a compression
  suffix such as `.jsonl.zst`
- `.psv` recognised as pipe-separated
- Nested JSON values are pretty-printed and tinted in the pager

### Changed

- Omitting the file argument now always reads standard input, as most
  command-line tools do, rather than printing help when nothing is piped
- `--delimiter` no longer defaults eagerly; it is only applied when given, so
  it can refine a format rather than override one

## [0.1.0] - 2026-07-27

### Added

- Record view: fields stacked vertically, clamped to a share of the screen, with
  `z` to expand one in place and `Enter` to open it in a full-screen pager
- Field pager with content search and less-style horizontal scrolling, which
  chops rather than wraps while shifted
- Table view for locating a row, with sampled column widths, a pinned row-number
  column and horizontal column scrolling
- Column-name search (`/`, `n`, `N`), row jump (`:N`, `:$`) and yank over OSC 52
- Markdown fence tinting in the pager
- Scrollable help overlay listing every binding and any load warnings
- Malformed rows are reconciled against the header and reported rather than
  failing the load
- Project scaffolding: Cargo manifest with the shared clippy lint set, Justfile,
  CI and release workflows, GPL-3.0 licence
- CLI surface: `--delimiter`, `--max-height`, `--no-wrap`, `--no-color`, and an
  optional file argument that falls back to stdin
- Design specification in `docs/design.md`
