#!/usr/bin/env bash
# Drive miolo inside a detached tmux session so the TUI can be exercised
# without a human at the keyboard: send keystrokes, capture the rendered
# screen as text, and assert on what came back.
#
#   scripts/tui.sh start [args...]     launch (defaults to the sample fixture)
#   scripts/tui.sh key <key>...        send keys by tmux name (j, C-d, Enter)
#   scripts/tui.sh type <text>         send literal text (":42", "/ship")
#   scripts/tui.sh snap [--color]      print the current screen
#   scripts/tui.sh resize <cols> <rows>
#   scripts/tui.sh alive               exit 0 if the program is still running
#   scripts/tui.sh stop
#
# Environment: MIOLO_SESSION, MIOLO_COLS, MIOLO_ROWS, MIOLO_SETTLE.
set -euo pipefail

SESSION="${MIOLO_SESSION:-miolo-test}"
COLS="${MIOLO_COLS:-100}"
ROWS="${MIOLO_ROWS:-30}"
SETTLE="${MIOLO_SETTLE:-0.4}"
FIXTURE="tests/fixtures/sample.csv"

die() {
    echo "tui.sh: $*" >&2
    exit 1
}

require_session() {
    tmux has-session -t "$SESSION" 2>/dev/null ||
        die "no session '$SESSION' — run 'scripts/tui.sh start' first"
}

cmd_start() {
    command -v tmux >/dev/null || die "tmux is not installed"
    cargo build --quiet || die "build failed"

    local bin="$PWD/target/debug/miolo"
    [ -x "$bin" ] || die "missing binary: $bin"

    local -a argv=("$bin")
    if [ "$#" -eq 0 ]; then
        argv+=("$FIXTURE")
    else
        argv+=("$@")
    fi

    tmux kill-session -t "$SESSION" 2>/dev/null || true

    # Start on a placeholder that never exits so remain-on-exit is set before
    # the real command can die; otherwise a crash takes the pane with it and
    # there is nothing left to capture.
    tmux new-session -d -s "$SESSION" -x "$COLS" -y "$ROWS" cat
    tmux set-option -t "$SESSION" remain-on-exit on >/dev/null
    tmux respawn-pane -k -t "$SESSION" "$(printf '%q ' "${argv[@]}")"
    sleep "$SETTLE"
}

cmd_key() {
    [ "$#" -gt 0 ] || die "key: expected at least one key name"
    require_session
    tmux send-keys -t "$SESSION" -- "$@"
    sleep "$SETTLE"
}

cmd_type() {
    [ "$#" -gt 0 ] || die "type: expected text"
    require_session
    tmux send-keys -t "$SESSION" -l -- "$*"
    sleep "$SETTLE"
}

cmd_snap() {
    require_session
    if [ "${1:-}" = "--color" ]; then
        tmux capture-pane -p -e -t "$SESSION"
    else
        tmux capture-pane -p -t "$SESSION"
    fi
}

cmd_resize() {
    [ "$#" -eq 2 ] || die "resize: expected <cols> <rows>"
    require_session
    tmux resize-window -t "$SESSION" -x "$1" -y "$2"
    sleep "$SETTLE"
}

cmd_alive() {
    require_session
    [ "$(tmux display-message -p -t "$SESSION" '#{pane_dead}')" = "0" ]
}

cmd_stop() {
    tmux kill-session -t "$SESSION" 2>/dev/null || true
}

case "${1:-}" in
    start) shift && cmd_start "$@" ;;
    key) shift && cmd_key "$@" ;;
    type) shift && cmd_type "$@" ;;
    snap) shift && cmd_snap "$@" ;;
    resize) shift && cmd_resize "$@" ;;
    alive) cmd_alive ;;
    stop) cmd_stop ;;
    *)
        sed -n '2,15p' "$0" | sed 's/^# \?//'
        exit 1
        ;;
esac
