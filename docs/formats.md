# miolo 0.2.0 — input formats

Extends [`design.md`](design.md) with non-CSV input. Everything downstream of
loading is unchanged: all formats produce the same `Table`, so the record view,
pager, table view and search need no changes.

## Scope

In scope:

- Format inferred from the file extension
- JSON files containing an array of objects
- JSONL / NDJSON, one object per line
- gzip and zstd decompression for every supported format
- `--format` to state the format explicitly, principally for stdin

Explicit non-goals:

- YAML, Parquet, xlsx. YAML has no real tabular use; Parquet drags in the Arrow
  stack and a columnar loader; xlsx needs sheet selection. None earns its
  complexity here.
- Becoming a JSON viewer. Nested values are pretty-printed and left alone —
  there is no drilling into them, no per-key navigation.
- Format *sniffing*. See below; this is a deliberate decision, not an omission.

## Resolution

Loading resolves an input to a `(Compression, Format)` pair, then dispatches.
The two are resolved by different means on purpose.

### Compression: detected from magic bytes

| Signature | Compression |
| --- | --- |
| `1f 8b` | gzip |
| `28 b5 2f fd` | zstd |
| anything else | none |

Detection, not inference. These are unambiguous binary signatures that no text
input can begin with, so the false-positive rate is nil.

Detecting rather than trusting the extension is what makes errors *better*:
handing gzipped bytes to a CSV parser yields a screen of binary noise, whereas
detecting the container yields "this is gzip, but its contents are not valid
CSV". It also means `curl … | miolo` works with no flag, and that a compressed
file with a misleading name still opens.

A recognised compression suffix (`.gz`, `.zst`) is stripped before the format
extension is examined, so `orders.json.gz` resolves to gzip + json.

### Format: from the extension, or stated

| Extension | Format |
| --- | --- |
| `.csv` | delimited, `,` |
| `.tsv`, `.tab` | delimited, tab |
| `.psv` | delimited, `\|` |
| `.json` | json |
| `.jsonl`, `.ndjson` | jsonl |
| anything else, or none | delimited, `,` |

For stdin the format is whatever `--format` says, defaulting to delimited —
which preserves the existing behaviour of `cat data.csv \| miolo -`.

`--format` always wins over the extension. `--delimiter` sets the separator for
delimited formats, and implies a delimited format when given alone.

### Why formats are not sniffed

Compression signatures identify; format sniffing guesses. A leading `[` or `{`
does not reliably separate JSON from JSONL from a CSV whose first cell happens
to start with a brace, and a wrong guess reports a parse failure against the
wrong format — confusing precisely when the user has already made a mistake.

Behaviour is therefore a function of the flags and the filename alone. Content
is never examined to choose a parser.

The accepted consequence: a CSV parser almost never fails, so opening a JSON
file that is named `.csv` produces a garbage single-column table rather than an
error. `--format` is the remedy.

## JSON and JSONL

Both are a sequence of objects, so they share one code path behind two front
ends: `.json` parses a whole array, `.jsonl` parses one object per line,
skipping blank lines.

### Columns

Columns are the **union of every object's keys, in first-seen order**. This
needs a pass over all objects before rendering, which is free given everything
is already in memory. A key absent from an object yields an empty field.

### Values

Rendered into the existing `Vec<Vec<String>>`; no per-cell type information is
stored, so a large CSV pays nothing for this feature.

| JSON value | Rendered as |
| --- | --- |
| string | the string itself, unquoted |
| number | the source token, exactly |
| `true` / `false` | `true` / `false` |
| `null` | empty — shown as `(empty)` |
| absent key | empty — shown as `(empty)` |
| object / array | pretty-printed JSON, multi-line |

`null` and an absent key are deliberately collapsed with the empty string.
Distinguishing them would need a per-cell kind on every cell in every file,
which is not worth a third more memory on a large CSV.

Numbers preserve their source token — `1.0` does not become `1`, and a large
integer does not lose digits — via `serde_json`'s `arbitrary_precision`.

Nested values pretty-print and then behave like any other tall field, which is
exactly what the record view exists for. The pager tints them as code, detected
at render time: a field whose text begins with `{` or `[` and spans several
lines is styled with the same treatment as a fenced block. A CSV cell that
happens to contain JSON is tinted too, which is a feature rather than a cost.

### Errors

Structural problems are fatal; bad individual records are not. This matches how
delimited input already behaves — one corrupt row never costs you the file.

| Problem | Outcome |
| --- | --- |
| Top-level JSON is not an array | error |
| Whole-document JSON syntax error | error |
| An array element is not an object | empty record, warned |
| A JSONL line is not an object | empty record, warned |
| A JSONL line is not valid JSON | empty record, warned |

A bad record becomes a row of empty fields rather than disappearing, so row
numbers continue to line up with positions in the file — row *n* is the *n*th
element or line whatever went wrong. Each one appends a `LoadWarning`, so the
status bar counts them and `?` lists them, exactly as for ragged CSV rows.

A whole-document syntax error stays fatal because there is nothing to recover:
`.json` is parsed as one value, so a stray brace invalidates everything after
it. `.jsonl` has no such problem, since each line stands alone.

## CLI

```
  -f, --format <FORMAT>   Input format [default: inferred from the extension]
                          [possible values: csv, tsv, psv, json, jsonl]
```

`--delimiter` is unchanged and applies to delimited formats.

## Architecture

The pure core holds. Each parser stays a function from bytes to a `Table`, and
decompression is a separate step in front of it, so the whole pipeline is
testable without touching the filesystem.

| Module | Role | Purity |
| --- | --- | --- |
| `source.rs` | Resolve `(Compression, Format)` from path, flags and magic bytes | pure |
| `decompress.rs` | Bytes to bytes | pure |
| `data/delimited.rs` | The existing CSV path, unchanged | pure |
| `data/json.rs` | Array of objects, and JSONL, to a `Table` | pure |
| `data/mod.rs` | Dispatch by format | pure |

`data.rs` becomes `data/` to stay well under the file size limit.

## Dependencies

All pure Rust, so cross-compiling the `aarch64-apple-darwin` release target
still needs no C toolchain:

| Crate | Purpose |
| --- | --- |
| `serde_json` | JSON parsing, with `preserve_order` and `arbitrary_precision` |
| `flate2` (`rust_backend`) | gzip decoding |
| `ruzstd` | zstd decoding — decode-only, which is all that is needed |

## Testing

- Resolution is a table-driven pure function: every extension, compression
  suffix, flag override and stdin combination.
- Round-trip tests decompressing fixtures of each format.
- JSON column-union ordering, absent keys and every value type.
- Each fatal case errors, and each recoverable case yields an empty record with
  a warning naming the right line — including a bad line surrounded by good
  ones, asserting the rows after it still align.
- Interactive checks per `CLAUDE.md`, with compressed and JSON fixtures added
  alongside `sample.csv`.
