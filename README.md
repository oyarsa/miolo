# miolo

A terminal viewer for CSV files whose columns contain long, multi-line text.

Standard CSV tools render a grid, which falls apart when a single field holds
several hundred lines of prose. `miolo` inverts the layout — one record per
screen, fields stacked vertically — so long text gets the full width of the
terminal and vertical space is spent on the field you care about.

> **Status:** early development. The interface is specified in
> [`docs/design.md`](docs/design.md); the implementation is not yet complete.

## Usage

```
miolo [OPTIONS] [FILE]

Arguments:
  [FILE]  CSV file to view. Use "-", or omit it with a pipe, to read stdin.

Options:
  -d, --delimiter <CHAR>  Field delimiter [default: ,]
  -m, --max-height <PCT>  Max field height, % of the record body [default: 40]
      --no-wrap           Start in truncate mode
      --no-color          Disable colour (NO_COLOR is also honoured)
  -h, --help              Print help
  -V, --version           Print version
```

The first row is always treated as the header.

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
