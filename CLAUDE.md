# Agent instructions

Project-specific instructions for working on miolo.

`docs/design.md` is the authoritative specification. Read it before changing
behaviour, and update it in the same commit when behaviour changes.

## Before Completing Any Task

1. Run `just fix` (formats code, autofixes clippy warnings, and runs tests)
2. Commit changes (do NOT bump version or tag unless explicitly asked)

## Releasing

Only release when explicitly requested (e.g. "release", "cut a release",
"bump version").

When releasing:

1. Bump the version in `Cargo.toml` (see Versioning below)
2. Update the release date in `src/cli.rs` (`long_version()`)
3. Add an entry to `CHANGELOG.md` describing what changed since the last release
4. `jj commit -m "v1.1.0: Release summary"`
5. `jj bookmark set master -r @-`
6. `jj git push`
7. `git tag v1.1.0 && git push --tags` (jj doesn't handle tags yet — pushing
   the tag triggers the release workflow automatically)

This project is not published to crates.io. Releases are tarballed binaries
only.

## Version Control

Use **jj** (Jujutsu), not git, for all version control operations. This is a
co-located repo, so a `.git` directory exists — use it only for tags.

- `jj status` - check working copy status
- `jj log` - view commit history
- `jj commit -m "message"` - create new commit with message for current change
- `jj squash` - squash current change into parent
- `jj bookmark set master` - move master bookmark to current commit
- `jj git push` - push to remote

Typical workflow:

1. Make changes (jj auto-tracks them)
2. `jj commit -m "Your commit message"` to commit the change
3. `jj bookmark set master` to update master
4. `jj git push` when ready to push

## Commits

- Do NOT add `Co-Authored-By` lines to commits
- Use natural, descriptive commit messages (no "conventional commits" format)
- Commit messages should explain what and why, not how
- Commit summary lines MUST be less than 70 characters

## Versioning

Standard semver: `MAJOR.MINOR.PATCH`

- **Major**: breaking changes or major new features
- **Minor**: new features, backwards compatible
- **Patch**: bug fixes
- Update `CHANGELOG.md` when bumping version

## Code Style

Beyond what clippy and rustfmt enforce:

- Favor pure functions over stateful methods
- Prefer immutable data structures
- Think functional programming over object-oriented/procedural
- Extract testable pure logic from impure functions (I/O, terminal, etc.)
- Avoid `.unwrap()` in production code; use `.expect("reason")` if a panic is
  truly justified, or propagate errors with `?`
- Every file stays under 1000 non-blank lines; split into focused modules
  before crossing that line
- MSRV is 1.88 and this is a binary, not a library, so there is no reason to
  write around modern language features. Let-chains in particular
  (`if let Some(f) = selected && f.is_expanded()`) suit the state and
  key-handling code better than nested `if let`s — use them.

### TUI-specific

The terminal is touched in exactly one place: the event loop in `main.rs`.
Everything else is a function from values to values.

- Key handling is `action_for(KeyEvent, Mode) -> Option<Action>` — a pure
  lookup, never a place for side effects
- State changes go through `update(&State, Action) -> State`
- Text layout (wrapping, clamping, elision) lives in `layout.rs` and never
  reads the terminal; it takes an explicit width and height
- Render functions draw from state and decide nothing

If a change requires the renderer to make a decision, that decision belongs in
`state.rs` or `layout.rs` instead.

## Testing

- No mocks — if something needs mocking, refactor to extract pure functions
- No slow tests — unit tests should be fast
- Prefer testing via pure functions that take inputs and return outputs
- Render tests use `ratatui::backend::TestBackend` and compare whole buffers
- When a test fails, evaluate whether the test or implementation is wrong
  before fixing
- Layout code needs edge-case coverage: zero and one-column widths, CJK and
  emoji, fields that are only newlines or only whitespace, and fields far
  taller than the viewport

## Interactive Testing

Unit tests cover pure functions and `TestBackend` covers rendered buffers, but
neither proves the program works when driven by an actual keyboard. `just fix`
passing is not evidence that a UI change behaves correctly.

`scripts/tui.sh` runs miolo in a detached tmux session, so keystrokes can be
sent and the rendered screen read back as plain text:

```sh
scripts/tui.sh start                # launch on tests/fixtures/sample.csv
scripts/tui.sh snap                 # print the current screen
scripts/tui.sh key j                # send a key, then snap again
scripts/tui.sh key C-d              # tmux key names: C-d, Enter, Escape, Down
scripts/tui.sh type :42             # literal text (":42", "/ship")
scripts/tui.sh resize 60 20         # re-check layout at another size
scripts/tui.sh alive                # exit 0 while the program is running
scripts/tui.sh stop                 # always, when finished
```

Rules:

- **Snap after every key.** Sending a batch and snapping once loses which
  keystroke caused what, which is exactly the information you need.
- **Always `stop`** when finished, so no stray session is left behind.
- `key` takes tmux key names; single characters are literal. Use `type` for
  multi-character literals like `:42`, or tmux will try to parse them as a key
  name.
- The pane survives the program exiting (`remain-on-exit`), so a panic or error
  message is still capturable. `alive` distinguishes "quit cleanly" from "still
  running" — use it to verify `q` actually quits.
- Default pane is 100x30; override with `MIOLO_COLS` / `MIOLO_ROWS`, or use
  `resize` mid-session. **Resize deliberately**: `--max-height` is a percentage
  of the body area, and the layout cache is keyed on width, so a resize is the
  cheapest way to catch stale-cache and rounding bugs.
- Any change touching layout should be checked at a minimum of two terminal
  sizes and in both wrap and truncate modes.

`just demo` opens the same fixture attached, for a human at the terminal.

### The fixture

`tests/fixtures/sample.csv` has 9 data rows, each deliberately awkward:

| Row | Exercises |
| --- | --- |
| 1 Ada Lovelace | Multi-paragraph prose with blank lines inside one field |
| 2 山田太郎 🎌 | CJK and emoji widths; empty `shipped_at` |
| 3 Kai Nakamura | Whitespace-only `notes` — must render `(whitespace)`, distinct from `(empty)` |
| 4 Bob "Bobby" Smith | Escaped quotes inside a quoted field |
| 5 Ana Ruiz | A Markdown ```json fence, for the pager's fence handling |
| 6 Marta Nowak | Short row (3 fields) — padded, and counted as a load warning |
| 7 Wei Chen | Long row (7 fields) — surplus columns named `+1`, `+2` |
| 8 Sam Okonkwo | Empty `notes` — must render `(empty)` |
| 9 Long Field Test | 60 numbered lines |

Row 9 exists so scroll position is verifiable at a glance: if the screen shows
`line 023` at the top, the viewport is unambiguous without counting rows. Use
it whenever checking `^d`/`^u`, `z`, `g`/`G`, or pager scrolling.

Rows 6 and 7 mean **the status bar should always show `⚠ 2 malformed`** on this
fixture. If it does not, the load-warning path has regressed.
