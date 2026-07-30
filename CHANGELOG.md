# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `TERM=dumb` disables colour, alongside `NO_COLOR` and `--no-color`
- The repository URL appears in `--help` and `--version`, so somewhere to read
  more and somewhere to report problems are one flag away

### Changed

- Run with no file and nothing piped, `miolo` now prints its help and exits
  instead of reading the terminal until `Ctrl-D` — a viewer waiting on an
  empty screen is indistinguishable from a hung one. Piped input is unaffected,
  and an explicit `-` still reads the terminal

### Fixed

- A save that failed part-way — running out of disk, say — could leave its
  staging file behind

## [0.3.0] - 2026-07-29

### Added

- Field editing: `e` opens the selected field. `Enter` inserts a newline, `^s`
  accepts, `Esc` discards after confirming
- The editor is inline in the record view — the field is edited in place, with
  the rest of the record still around it — and full screen when opened from the
  pager, where the field was already being read that way. Editing lifts the
  field height cap, so the field being typed into always shows all of itself
- `W` writes the table back to its file, through a temporary file and a rename
  so an interrupted save cannot leave a half-written file
- `u` undoes the last committed edit
- Unsaved changes are marked in the status bar, a `W save` hint joins the
  footer while there is something to write and somewhere to write it, and `q`
  refuses to quit while changes exist — `W` writes, `Q` quits anyway
- Closing the editor returns to the view it was opened from, so `e` in the
  pager comes back to the pager rather than dropping to the record view

### Changed

- Writing is refused, with the reason, for standard input, compressed input,
  JSON and JSONL, and for a file that changed on disk since it was loaded. The
  reason is reported when the editor opens rather than when a save is attempted
- Writing a delimited file reflects the table rather than the bytes that were
  read: rows padded at load are written back square and quoting is normalised.
  The status line says so when the input had load warnings
- `u` now undoes rather than being unbound; `^u` still scrolls
- `^c` quits without the unsaved-changes guard, as an interrupt should

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
