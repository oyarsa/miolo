//! Key and mouse decoding.
//!
//! A pure lookup from input event to [`Action`]. No side effects live here, so
//! the whole binding table is testable as a plain function.
//!
//! Most keys mean the same thing in every mode; the transition function
//! interprets a generic direction against whichever view is active.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::state::{Action, Focus, Mode};

/// Lines a mouse wheel notch scrolls.
const WHEEL_LINES: usize = 3;

/// Decode a key press into an action, or `None` if the key is unbound.
///
/// The focus comes first because it changes what a key *is*, not just what it
/// does: while a prompt or the editor is open almost every key is text.
pub fn action_for(key: KeyEvent, mode: Mode, focus: Focus) -> Option<Action> {
    // Windows reports press and release; only act on press.
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    match focus {
        Focus::Prompt => prompt_key(key.code, ctrl),
        Focus::Edit => edit_key(key, ctrl),
        Focus::Confirm => confirm_key(key.code, ctrl),
        Focus::Normal => normal_key(key.code, ctrl, mode),
    }
}

fn prompt_key(code: KeyCode, ctrl: bool) -> Option<Action> {
    match code {
        KeyCode::Char('c') if ctrl => Some(Action::PromptCancel),
        KeyCode::Char(c) => Some(Action::PromptPush(c)),
        KeyCode::Backspace => Some(Action::PromptPop),
        KeyCode::Enter => Some(Action::PromptSubmit),
        KeyCode::Esc => Some(Action::PromptCancel),
        _ => None,
    }
}

/// Keys inside the editor.
///
/// `Enter` inserts a newline rather than accepting, because the fields this
/// viewer exists for are several paragraphs long: the common operation gets
/// the obvious key, and accepting gets a chord. Nothing here quits — `^c`
/// abandons the edit, so no single keystroke can end the session with a
/// half-typed field in it.
fn edit_key(key: KeyEvent, ctrl: bool) -> Option<Action> {
    // Alt-chords are left unbound rather than treated as text: terminals
    // disagree about whether they arrive at all.
    let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();

    match key.code {
        KeyCode::Char('s') if ctrl => Some(Action::EditCommit),
        KeyCode::Char('c') if ctrl => Some(Action::EditCancel),
        KeyCode::Char('d') if ctrl => Some(Action::HalfDown),
        KeyCode::Char('u') if ctrl => Some(Action::HalfUp),
        KeyCode::Char(c) if plain => Some(Action::EditInsert(c)),

        KeyCode::Enter => Some(Action::EditNewline),
        KeyCode::Tab => Some(Action::EditInsert('\t')),
        KeyCode::Backspace => Some(Action::EditBackspace),
        KeyCode::Delete => Some(Action::EditDelete),
        KeyCode::Esc => Some(Action::EditCancel),

        KeyCode::Left => Some(Action::Left),
        KeyCode::Right => Some(Action::Right),
        KeyCode::Up => Some(Action::Up),
        KeyCode::Down => Some(Action::Down),
        KeyCode::Home => Some(Action::First),
        KeyCode::End => Some(Action::Last),
        KeyCode::PageDown => Some(Action::HalfDown),
        KeyCode::PageUp => Some(Action::HalfUp),
        _ => None,
    }
}

/// Keys answering "discard changes?". Only an explicit yes discards.
fn confirm_key(code: KeyCode, ctrl: bool) -> Option<Action> {
    match code {
        _ if ctrl => Some(Action::ConfirmNo),
        KeyCode::Char('y' | 'Y') => Some(Action::ConfirmYes),
        KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(Action::ConfirmNo),
        _ => None,
    }
}

fn normal_key(code: KeyCode, ctrl: bool, mode: Mode) -> Option<Action> {
    match code {
        KeyCode::Char('d') if ctrl => Some(Action::HalfDown),
        KeyCode::Char('u') if ctrl => Some(Action::HalfUp),
        // Unconditional, so a terminal that has stopped responding to anything
        // else still exits. Unsaved changes are lost, as they are for any
        // program interrupted this way.
        KeyCode::Char('c') if ctrl => Some(Action::ForceQuit),

        KeyCode::Char('j') | KeyCode::Down => Some(Action::Down),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::Up),
        KeyCode::Char('l') | KeyCode::Right => Some(Action::Right),
        KeyCode::Char('h') | KeyCode::Left => Some(Action::Left),
        KeyCode::Char('H') => Some(Action::ColumnLeft),
        KeyCode::Char('L') => Some(Action::ColumnRight),
        KeyCode::Char('g') => Some(Action::First),
        KeyCode::Char('G') => Some(Action::Last),

        KeyCode::Enter => Some(Action::Enter),
        KeyCode::Esc => Some(Action::Back),
        KeyCode::Char('t') => Some(Action::ToggleTable),
        KeyCode::Char('z') => Some(Action::ToggleExpand),
        KeyCode::Char('w') => Some(Action::ToggleWrap),
        KeyCode::Char('?') => Some(Action::ToggleHelp),
        KeyCode::Char('/') => Some(Action::BeginSearch),
        KeyCode::Char(':') => Some(Action::BeginJump),
        KeyCode::Char('n') => Some(Action::NextMatch),
        KeyCode::Char('N') => Some(Action::PrevMatch),
        KeyCode::Char('y') => Some(Action::Yank),

        KeyCode::Char('e') => Some(Action::BeginEdit),
        KeyCode::Char('u') => Some(Action::Undo),
        KeyCode::Char('W') => Some(Action::Save),
        KeyCode::Char('Q') => Some(Action::ForceQuit),

        // In the pager `q` steps back to the record rather than exiting, so
        // reading a field is never one keystroke away from losing the session.
        KeyCode::Char('q') => Some(if mode == Mode::Record {
            Action::Quit
        } else {
            Action::Back
        }),
        _ => None,
    }
}

/// Decode a mouse event into an action.
pub fn action_for_mouse(kind: MouseEventKind, row: u16) -> Option<Action> {
    match kind {
        MouseEventKind::ScrollDown => Some(Action::ScrollDown(WHEEL_LINES)),
        MouseEventKind::ScrollUp => Some(Action::ScrollUp(WHEEL_LINES)),
        MouseEventKind::Down(_) => Some(Action::Click(usize::from(row))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn record(code: KeyCode) -> Option<Action> {
        action_for(key(code), Mode::Record, Focus::Normal)
    }

    fn editing(code: KeyCode) -> Option<Action> {
        action_for(key(code), Mode::Edit, Focus::Edit)
    }

    fn confirming(code: KeyCode) -> Option<Action> {
        action_for(key(code), Mode::Edit, Focus::Confirm)
    }

    #[test]
    fn vim_keys_map_to_directions() {
        assert_eq!(record(KeyCode::Char('j')), Some(Action::Down));
        assert_eq!(record(KeyCode::Char('k')), Some(Action::Up));
        assert_eq!(record(KeyCode::Char('l')), Some(Action::Right));
        assert_eq!(record(KeyCode::Char('h')), Some(Action::Left));
    }

    #[test]
    fn arrows_mirror_the_vim_keys() {
        assert_eq!(record(KeyCode::Down), Some(Action::Down));
        assert_eq!(record(KeyCode::Up), Some(Action::Up));
        assert_eq!(record(KeyCode::Right), Some(Action::Right));
        assert_eq!(record(KeyCode::Left), Some(Action::Left));
    }

    #[test]
    fn control_keys_scroll_by_half_a_page() {
        assert_eq!(
            action_for(ctrl('d'), Mode::Record, Focus::Normal),
            Some(Action::HalfDown)
        );
        assert_eq!(
            action_for(ctrl('u'), Mode::Record, Focus::Normal),
            Some(Action::HalfUp)
        );
    }

    #[test]
    fn plain_letters_are_not_confused_with_control_chords() {
        assert_eq!(record(KeyCode::Char('d')), None);
        assert_eq!(
            record(KeyCode::Char('u')),
            Some(Action::Undo),
            "plain u undoes; only ^u scrolls"
        );
    }

    #[test]
    fn quit_only_exits_from_the_record_view() {
        assert_eq!(record(KeyCode::Char('q')), Some(Action::Quit));
        assert_eq!(
            action_for(key(KeyCode::Char('q')), Mode::Pager, Focus::Normal),
            Some(Action::Back),
            "q in the pager steps back rather than exiting"
        );
        assert_eq!(
            action_for(key(KeyCode::Char('q')), Mode::Table, Focus::Normal),
            Some(Action::Back)
        );
    }

    #[test]
    fn control_c_always_quits() {
        assert_eq!(
            action_for(ctrl('c'), Mode::Pager, Focus::Normal),
            Some(Action::ForceQuit),
            "unconditionally, so unsaved changes cannot trap the user"
        );
    }

    #[test]
    fn capital_q_quits_past_the_unsaved_guard() {
        assert_eq!(record(KeyCode::Char('Q')), Some(Action::ForceQuit));
    }

    #[test]
    fn capitals_scroll_columns() {
        assert_eq!(record(KeyCode::Char('H')), Some(Action::ColumnLeft));
        assert_eq!(record(KeyCode::Char('L')), Some(Action::ColumnRight));
    }

    #[test]
    fn prompts_capture_text() {
        assert_eq!(
            action_for(key(KeyCode::Char('j')), Mode::Record, Focus::Prompt),
            Some(Action::PromptPush('j')),
            "letters are text, not navigation, while prompting"
        );
        assert_eq!(
            action_for(key(KeyCode::Enter), Mode::Record, Focus::Prompt),
            Some(Action::PromptSubmit)
        );
        assert_eq!(
            action_for(key(KeyCode::Backspace), Mode::Record, Focus::Prompt),
            Some(Action::PromptPop)
        );
        assert_eq!(
            action_for(key(KeyCode::Esc), Mode::Record, Focus::Prompt),
            Some(Action::PromptCancel)
        );
    }

    #[test]
    fn prompt_captures_keys_that_are_commands_otherwise() {
        for c in ['q', '/', ':', 'z', 'w', '?'] {
            assert_eq!(
                action_for(key(KeyCode::Char(c)), Mode::Record, Focus::Prompt),
                Some(Action::PromptPush(c)),
                "{c} must be typeable in a prompt"
            );
        }
    }

    #[test]
    fn control_c_escapes_a_prompt() {
        assert_eq!(
            action_for(ctrl('c'), Mode::Record, Focus::Prompt),
            Some(Action::PromptCancel)
        );
    }

    #[test]
    fn e_opens_the_editor() {
        assert_eq!(record(KeyCode::Char('e')), Some(Action::BeginEdit));
        assert_eq!(
            action_for(key(KeyCode::Char('e')), Mode::Pager, Focus::Normal),
            Some(Action::BeginEdit),
            "editable from the field you are reading, too"
        );
    }

    #[test]
    fn writing_and_undoing_have_their_own_keys() {
        assert_eq!(record(KeyCode::Char('W')), Some(Action::Save));
        assert_eq!(record(KeyCode::Char('u')), Some(Action::Undo));
    }

    #[test]
    fn the_editor_treats_letters_as_text() {
        for c in ['q', 'w', 'j', 'e', 'W', 'Q', ':', '/'] {
            assert_eq!(
                editing(KeyCode::Char(c)),
                Some(Action::EditInsert(c)),
                "{c} must be typeable in a field"
            );
        }
    }

    #[test]
    fn enter_inserts_a_newline_and_a_chord_accepts() {
        assert_eq!(editing(KeyCode::Enter), Some(Action::EditNewline));
        assert_eq!(
            action_for(ctrl('s'), Mode::Edit, Focus::Edit),
            Some(Action::EditCommit)
        );
    }

    #[test]
    fn the_editor_never_quits_outright() {
        assert_eq!(
            action_for(ctrl('c'), Mode::Edit, Focus::Edit),
            Some(Action::EditCancel),
            "^c abandons the field rather than the session"
        );
        assert_eq!(editing(KeyCode::Esc), Some(Action::EditCancel));
    }

    #[test]
    fn arrows_move_the_caret_while_editing() {
        assert_eq!(editing(KeyCode::Left), Some(Action::Left));
        assert_eq!(editing(KeyCode::Right), Some(Action::Right));
        assert_eq!(editing(KeyCode::Up), Some(Action::Up));
        assert_eq!(editing(KeyCode::Down), Some(Action::Down));
        assert_eq!(editing(KeyCode::Home), Some(Action::First));
        assert_eq!(editing(KeyCode::End), Some(Action::Last));
    }

    #[test]
    fn the_editor_edits_text_with_the_editing_keys() {
        assert_eq!(editing(KeyCode::Backspace), Some(Action::EditBackspace));
        assert_eq!(editing(KeyCode::Delete), Some(Action::EditDelete));
        assert_eq!(editing(KeyCode::Tab), Some(Action::EditInsert('\t')));
    }

    #[test]
    fn alt_chords_are_not_text() {
        let alt = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT);
        assert_eq!(
            action_for(alt, Mode::Edit, Focus::Edit),
            None,
            "terminals disagree about Alt, so it stays unbound"
        );
    }

    #[test]
    fn shifted_letters_are_still_text() {
        let shifted = KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT);
        assert_eq!(
            action_for(shifted, Mode::Edit, Focus::Edit),
            Some(Action::EditInsert('A'))
        );
    }

    #[test]
    fn only_an_explicit_yes_discards() {
        assert_eq!(confirming(KeyCode::Char('y')), Some(Action::ConfirmYes));
        assert_eq!(confirming(KeyCode::Char('Y')), Some(Action::ConfirmYes));
        assert_eq!(confirming(KeyCode::Char('n')), Some(Action::ConfirmNo));
        assert_eq!(confirming(KeyCode::Esc), Some(Action::ConfirmNo));
        assert_eq!(
            confirming(KeyCode::Enter),
            None,
            "a stray Enter must not throw the field away"
        );
        assert_eq!(
            action_for(ctrl('c'), Mode::Edit, Focus::Confirm),
            Some(Action::ConfirmNo)
        );
    }

    #[test]
    fn search_and_jump_open_prompts() {
        assert_eq!(record(KeyCode::Char('/')), Some(Action::BeginSearch));
        assert_eq!(record(KeyCode::Char(':')), Some(Action::BeginJump));
        assert_eq!(record(KeyCode::Char('n')), Some(Action::NextMatch));
        assert_eq!(record(KeyCode::Char('N')), Some(Action::PrevMatch));
    }

    #[test]
    fn releases_are_ignored() {
        let mut event = key(KeyCode::Char('j'));
        event.kind = KeyEventKind::Release;
        assert_eq!(action_for(event, Mode::Record, Focus::Normal), None);
    }

    #[test]
    fn unbound_keys_do_nothing() {
        assert_eq!(record(KeyCode::Char('%')), None);
        assert_eq!(record(KeyCode::F(5)), None);
    }

    #[test]
    fn wheel_scrolls_and_clicks_select() {
        assert_eq!(
            action_for_mouse(MouseEventKind::ScrollDown, 0),
            Some(Action::ScrollDown(WHEEL_LINES))
        );
        assert_eq!(
            action_for_mouse(MouseEventKind::ScrollUp, 0),
            Some(Action::ScrollUp(WHEEL_LINES))
        );
        assert_eq!(
            action_for_mouse(MouseEventKind::Down(crossterm::event::MouseButton::Left), 7),
            Some(Action::Click(7))
        );
    }

    #[test]
    fn mouse_drag_is_ignored() {
        assert_eq!(
            action_for_mouse(MouseEventKind::Drag(crossterm::event::MouseButton::Left), 3),
            None
        );
    }
}
