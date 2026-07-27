# miolo

A terminal viewer for tabular files whose columns contain long, multi-line
text — CSV, TSV, JSON and JSONL, compressed or not.

Standard CSV tools render a grid, which falls apart when a single field holds
several hundred lines of prose. `miolo` inverts the layout — one record per
screen, fields stacked vertically — so long text gets the full width of the
terminal and vertical space is spent on the field you care about.

> **Status:** usable. The interface is specified in
> [`docs/design.md`](docs/design.md).

## Usage

```
miolo [OPTIONS] [FILE]

Arguments:
  [FILE]  File to view. Use "-", or omit it with a pipe, to read stdin.

Options:
  -f, --format <FORMAT>   Input format [default: inferred from the extension]
                          [possible values: csv, tsv, psv, json, jsonl]
  -d, --delimiter <CHAR>  Field delimiter for separated values [default: ,]
  -m, --max-height <PCT>  Max field height, % of the record body [default: 40]
      --no-wrap           Start in truncate mode
      --no-color          Disable colour (NO_COLOR is also honoured)
  -h, --help              Print help
  -V, --version           Print version
```

For separated values the first row is always the header.

## Formats

The format comes from the file extension — `.csv`, `.tsv`, `.tab`, `.psv`,
`.json`, `.jsonl`, `.ndjson` — or from `--format`, which is how you tell miolo
what is arriving on stdin. Anything unrecognised is read as comma-separated.

gzip and zstd are unwrapped automatically, detected from the file's leading
bytes rather than its name, so `orders.jsonl.zst` and `curl … | miolo` both
work without saying anything:

```sh
miolo orders.jsonl.zst
zstdcat orders.jsonl.zst | miolo -f jsonl
```

JSON input must be an array of objects; JSONL is one object per line. Columns
are the union of every object's keys. Nested objects and arrays are
pretty-printed into the field and left at that — miolo shows JSON, it does not
navigate it.

A malformed record does not cost you the file: it becomes an empty row with a
warning, listed under `?`, exactly like a ragged CSV row.

## Keys

`h`/`l` move between rows, `j`/`k` between fields. `z` expands the selected
field in place; `Enter` opens it in a full-screen pager with content search.
`/` searches column names, `:42` jumps to a row, `t` toggles the table view,
`y` yanks the selected field, and `?` lists every binding.

## Development

Requires Rust 1.88 or later.

```sh
just fix        # format, autofix clippy warnings, run tests
just check-all  # lint and test without modifying anything
```

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
