# miolo — design

A terminal viewer for CSV files whose columns contain long, multi-line text.

Input formats beyond CSV — JSON, JSONL, and compressed files — are specified
separately in [`formats.md`](formats.md).

## Problem

Standard CSV tools render a grid. That fails when a single field holds several
hundred lines of prose: the row becomes unreadable, and horizontal scrolling
through a table is a poor way to read text. `miolo` inverts the layout — one
record per screen, fields stacked vertically — so long text gets the full width
of the terminal and vertical space is spent on the field you care about.

## Scope

In scope for v1:

- Read-only viewing of a single CSV file (or stdin)
- Record view (primary), field pager, table view, help overlay
- Column-name search, jump-to-row, yank, wrap toggle, mouse

Explicit non-goals for v1:

- Editing or writing CSV
- Sorting or filtering rows
- Full-text search across rows (you can search column *names*, and content
  within a single field via the pager — but not "which row mentions SPRING20")
- Hiding or reordering columns
- Config file (flags only)
- Multiple files or tabs
- Publishing to crates.io

## Data model

The whole file is loaded into memory at startup. No lazy indexing, no mmap.

```
Table {
    headers:  Vec<String>,
    rows:     Vec<Vec<String>>,
    warnings: Vec<LoadWarning>,
}
```

- The first row is **always** the header. There is no `--no-header`.
- Data rows are numbered from 1; the header is not a row.
- Parsing uses the `csv` crate with `flexible(true)`, so quoted fields with
  embedded newlines, commas and quotes are handled correctly.

### Malformed input

Loading never fails on bad data, only on I/O errors.

- **Short rows** are padded with empty fields.
- **Long rows** get synthetic headers `+1`, `+2`, … for the surplus fields.
- **Invalid UTF-8** goes through `String::from_utf8_lossy`.

Each occurrence appends a `LoadWarning { row, kind }`. The status bar shows a
counter; `?` opens the help overlay, which lists the affected rows.

```
 orders.csv                    row 42/1,337   ⚠ 3 malformed   RECORD
```

### Text normalisation

Applied for **display only** — yank always copies the raw field text.

- `\r\n` and lone `\r` normalise to `\n`
- Tabs expand to 4 spaces
- An empty field renders as a dimmed `(empty)`
- A field of only whitespace renders as a dimmed `(whitespace)`, so the two
  cases are distinguishable

## Views

### Record view (primary)

One row per screen. Fields stacked vertically, each clamped to a maximum
height. The selected field is marked with `▌` in the gutter.

```
 orders.csv                             row 42/1,337      RECORD
────────────────────────────────────────────────────────────────
   order_id                                                 1/8
   8f3a91c2-4c7e-4b1a-9f21-77d0e5a6b3ee

 ▌ customer                                                 2/8
 ▌ Ada Lovelace <ada@example.com>

   notes                                       3/8   1-6 of 312
   Customer called about the delayed shipment. They were told
   at checkout that it would arrive Tuesday, but the tracking
   number was only issued on Thursday evening.

   Follow-up: offered a partial refund (20%), declined. Asked
   instead for expedited shipping on the replacement unit.
   ⋯ 306 more lines · <Enter> to open ⋯

   total_cents                                              4/8
   12995

   shipped_at                                               5/8
   (empty)
────────────────────────────────────────────────────────────────
 h/l row  j/k field  ^d/^u scroll  ↵ open  / search  ? help
```

`j`/`k` move the selection between fields; the body scrolls as needed to keep
the selection visible. `^d`/`^u` scroll the body by half a page. There is no
nested scrolling — `^d` always means "half a page of the record".

### Field height

`--max-height <PCT>` sets the cap as a percentage of the record body area
(the screen minus the header and footer bars). Default **40%**, floored at
**3 lines** so small terminals stay usable.

The cap is a maximum, not a fixed height:

- A field shorter than the cap renders at its natural height.
- A field taller than the cap is truncated, with a `⋯ N more lines` footer.
- The cap is **never** exceeded automatically, even when the whole record
  would otherwise fit on screen with room to spare.

Two ways past the cap:

- **`z`** expands the selected field to its full height *in place*. Surrounding
  fields stay in the document and the body scrolls through the expanded field
  normally. `z` again collapses it. Only one field is expanded at a time.
- **`Enter`** opens the field pager.

### Field pager

Full-screen view of a single field. Wraps by default — truncating prose you
are trying to read makes no sense — with less-style horizontal scrolling
available when you need it.

```
 orders.csv · row 42 · notes                     lines 24-48/312
────────────────────────────────────────────────────────────────
 instead for expedited shipping on the replacement unit. The
 warehouse confirmed stock on hand, so we routed it through
 the Leeds depot rather than waiting on the Tuesday truck.

 2024-03-19 — customer emailed again, this time about the
 invoice not reflecting the discount code (SPRING20).
────────────────────────────────────────────────────────────────
 j/k line  ←/→ shift  ^d/^u page  g/G top/end  / search  y yank  q back
```

`/` here searches the **content** of this field, not column names.

Horizontal scrolling follows `less`: `←`/`→` (aliased to `h`/`l`) shift the
view by half a screen width, and **while the offset is non-zero the pager
behaves as though lines were chopped**, exactly as less switches to `-S` mode
when you scroll. Returning to offset 0 restores wrapping. `h`/`l` do *not*
navigate between rows here — leaving the pager is `q`.

`less` binds `h` to help; we do not, since `?` already serves that purpose
everywhere.

### Table view

Secondary; `t` toggles. For locating a row rather than reading one.

```
 orders.csv                                    rows 38-44/1,337  TABLE
──────────────────────────────────────────────────────────────────────
  #      order_id     customer          notes                   total
  40     8f3a91c2…    Bob Smith         Shipment was late a…   4 200
  41     2b1d77aa…    Ana Ruiz          ⏎ Wrong size deliv…    8 990
▌ 42     9c40e1fb…    Ada Lovelace      Customer called ab…   12 995
  43     0aa71c33…    Kai Nakamura      (empty)                 3 100
──────────────────────────────────────────────────────────────────────
 ↵ open record  j/k row  H/L column  / search  ? help
```

- Column widths are sampled from the first ~1000 rows and capped, with `…`
  elision. Computed once at startup, since they depend only on the data.
- Embedded newlines render as `⏎` so one row is always one line.
- The `#` (row number) column is pinned; `H`/`L` scroll the data columns.
- Selection is shared with the record view in both directions: `Enter` opens
  the selected row as a record, and `t` from a record returns to the table with
  that row selected.

## Search

`/` searches **column names**, not content — except in the pager, where it
searches that field's content.

- Plain substring matching, case-insensitive (vim-style, not fuzzy).
- Jumps to the first match; `n`/`N` cycle forward/backward. It does not filter
  or hide non-matching fields.
- The term persists across row changes, so `n` keeps landing on the same field
  as you move through rows.
- In the record view, a match moves the field selection. In the table view, it
  scrolls horizontally to bring that column into view.

```
 ▌ shipped_at                                              7/8
 ▌ 2024-03-19T14:02:11Z
────────────────────────────────────────────────────────────────
 /ship_                                            2 matches  n/N
```

`:42` jumps to a row by number, in both the record and table views. `:$` jumps
to the last row. Out-of-range input is rejected with a message rather than
clamping silently.

`q` only exits from the record view. In the pager, table and help overlay it
steps back one level, so reading a field is never one keystroke away from
ending the session. `Ctrl-C` quits from anywhere.

## Markdown fences

Fields are plain text unless they literally contain Markdown code fences:

    ```json
    {"order": 42}
    ```

Where a fence is present, the fenced region gets a distinct background and the
fence markers themselves are dimmed. **No tokenising syntax highlighter** in
v1 — no `syntect`, no language grammars. The language tag is displayed but not
acted upon.

This applies in the pager only. The record view stays plain, since it is for
scanning.

## Yank

`y` copies the selected field's **full raw text** — pre-normalisation, not
just the visible portion — via an OSC 52 escape sequence. No `arboard`, no X11
or Wayland dependency, and it works over SSH.

- Content beyond ~64 KB is truncated, with the status bar reporting how much
  was dropped.
- There is no whole-row yank.
- In tmux this needs `set -g set-clipboard on`.

## Mouse

Always enabled; there is no flag or toggle to disable it.

- Wheel scrolls the current view
- Click selects a field (record view) or a row (table view)
- No drag, no selection, no double-click handling

Mouse capture suppresses the terminal's native click-drag text selection. The
standard escape hatch is holding **Shift** while dragging, which works in
xterm, iTerm2, kitty, WezTerm and Alacritty.

## Keybindings

Modal, but only lightly — most keys mean the same thing everywhere.

### Global

| Key | Action |
| --- | --- |
| `?` | Help overlay |
| `q` | Quit (record view only); steps back from the pager, table and overlay |
| `Esc` | Back one level; cancel a search or `:` prompt |
| `:42` | Jump to row 42 (`:$` = last row) |
| `/` | Search (column names; field content in the pager) |
| `n` / `N` | Next / previous match |
| `w` | Toggle wrap ↔ truncate (global) |
| `t` | Toggle record ↔ table view |

### Record view

| Key | Action |
| --- | --- |
| `h` `l` / `←` `→` | Previous / next row |
| `j` `k` / `↓` `↑` | Previous / next field |
| `^d` / `^u` | Scroll body half a page |
| `g` / `G` | First / last field |
| `z` | Expand or collapse the selected field in place |
| `Enter` | Open the selected field in the pager |
| `y` | Yank the selected field |

### Pager

| Key | Action |
| --- | --- |
| `j` `k` | Scroll one line |
| `^d` / `^u` | Scroll half a page |
| `←` `→` / `h` `l` | Shift horizontally half a screen width (chops while shifted) |
| `g` / `G` | Top / bottom |
| `/` `n` `N` | Search this field's content |
| `y` | Yank the field |
| `q` / `Esc` | Back to the record view |

### Table view

| Key | Action |
| --- | --- |
| `j` `k` | Previous / next row |
| `H` / `L` | Scroll columns left / right |
| `g` / `G` | First / last row |
| `Enter` | Open this row in the record view |

`g`/`G` is contextual — first/last along whatever the current view's primary
axis is (fields, lines, rows).

## Wrap and truncate

`w` toggles globally and persists across rows and views. Turning wrap back on
resets any horizontal offset in the pager to 0.

In truncate mode, over-long lines are cut at the right edge with `…`. In the
record and table views there is **no horizontal scrolling within a field** —
truncate mode is for scanning, `Enter` is the way to read, and this keeps
`h`/`l` free for row navigation. The pager is the exception: it is a reader, so
it gets less-style horizontal scrolling as described above.

Wrapping is Unicode-aware (`unicode-width`), so CJK and emoji do not break the
layout.

## CLI

```
miolo [OPTIONS] [FILE]

Arguments:
  [FILE]  File to view. Omit it, or pass "-", to read standard input.

Options:
  -d, --delimiter <CHAR>  Field delimiter [default: ,]
  -m, --max-height <PCT>  Max field height, % of the record body [default: 40]
      --no-wrap           Start in truncate mode
      --no-color          Disable colour (NO_COLOR is also honoured)
  -h, --help              Print help
  -V, --version           Print version
```

A missing path means standard input, as for most command-line tools. Input is
read to EOF before the terminal is put into raw mode, so a slow pipe only
delays startup. With no path and nothing piped, that means waiting on the
terminal until `Ctrl-D` — the same as `cat` with no arguments.

Reading from stdin means stdin is not the terminal, so the event loop reopens
`/dev/tty` for keyboard input.

## Rendering

Colours come from the terminal's 16 ANSI slots, not a hardcoded truecolor
palette, so the viewer follows the user's theme. `NO_COLOR` and `--no-color`
fall back to bold/dim/reverse attributes only.

Wrapped-line layout is recomputed each frame and deliberately **not** cached.
The concern that a long field would be expensive to re-wrap per keypress did
not survive measurement: assembling the record body for a 60-line field takes
about 0.7ms, comfortably inside a frame. A cache would have been complexity
bought with nothing.

Column widths are the opposite case. Sampling them costs ~86ms on a 50k-row
file, and they were originally computed per frame, which made the table view
unusable on large input. They depend only on the data, never on the terminal
size, so they are computed once at startup and held in the render context —
a resize does not invalidate them.

## Architecture

A pure core with a thin impure shell, matching the house style: pure functions
over stateful methods, immutable data, no `.unwrap()` in production code.

| Module | Role | Purity |
| --- | --- | --- |
| `main.rs` | Terminal setup/teardown, event loop | impure |
| `cli.rs` | `clap` definitions | pure |
| `data.rs` | Load and normalise CSV from a path or stdin | I/O boundary |
| `state.rs` | `State`, `Action`, and `update(&State, Action) -> State` | pure |
| `keys.rs` | `action_for(KeyEvent, Mode) -> Option<Action>` | pure |
| `layout.rs` | Wrapping, clamping, `FieldLayout`, the layout cache | pure |
| `search.rs` | Substring matching and match cycling | pure |
| `markdown.rs` | Fence detection → styled spans | pure |
| `clipboard.rs` | OSC 52 emission | impure |
| `ui/*.rs` | Render one view each; no decisions | pure-ish |

The event loop is the only place that touches the terminal. Everything a test
would want to assert on — key mapping, state transitions, text layout, search —
is a pure function taking values and returning values.

Every module stays under 1000 non-blank lines; `ui/` is split per view partly
for that reason.

## Testing

- Table-driven unit tests over the pure functions: key mapping per mode, state
  transitions, wrapping and clamping edge cases (zero-width terminals, CJK,
  fields of only newlines), search cycling and wraparound, CSV normalisation
  including every malformed-input path.
- Snapshot tests of rendered frames via `ratatui::backend::TestBackend`,
  comparing whole buffers.
- No mocks. If something appears to need one, extract the pure part instead.
- No slow tests.

## Dependencies

| Crate | Purpose |
| --- | --- |
| `ratatui` + `crossterm` | TUI rendering and events |
| `csv` | Parsing, including quoted embedded newlines |
| `unicode-width` | Correct widths for CJK and emoji |
| `textwrap` | Line breaking |
| `clap` (derive) | CLI |
| `anyhow` / `thiserror` | Errors |

Deliberately absent: `syntect` (too heavy for v1's fence handling), `arboard`
(OSC 52 instead), `memchr` (not needed without lazy indexing).

Lints mirror fleche: `clippy::all` and `clippy::pedantic` at `warn`, plus the
same nursery selection, with `too_many_lines`, `too_many_arguments`,
`format_push_string` and `struct_excessive_bools` allowed. Edition 2024.

## Packaging

- Licence: GPL-3.0-or-later
- CI builds release binaries for `x86_64-unknown-linux-gnu`,
  `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin`, tarballed and
  attached to the tag — the same matrix as fleche, minus the crates.io publish
  job
- `Justfile` with the same recipes: `build`, `run`, `release`, `install`,
  `test`, `clippy`, `clippy-fix`, `fmt`, `fmt-check`, `lint`, `check-all`,
  `fix`

## Deferred to a later version

Full-text search across rows is the most likely first addition — it is the one
capability the current search model cannot express. Sorting, filtering, column
hiding and a config file follow.
