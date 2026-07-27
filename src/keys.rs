//! Key and mouse decoding.
//!
//! A pure lookup from input event to [`Action`]. No side effects live here, so
//! the whole binding table is testable as a plain function.
//!
//! Most keys mean the same thing in every mode; the transition function
//! interprets a generic direction against whichever view is active.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};

use crate::state::{Action, Mode};

/// Lines a mouse wheel notch scrolls.
const WHEEL_LINES: usize = 3;

/// Decode a key press into an action, or `None` if the key is unbound.
///
/// While a prompt is open almost every key becomes text, so that case is
/// handled first and separately.
pub fn action_for(key: KeyEvent, mode: Mode, prompting: bool) -> Option<Action> {
    // Windows reports press and release; only act on press.
    if key.kind == KeyEventKind::Release {
        return None;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    if prompting {
        return match key.code {
            KeyCode::Char('c') if ctrl => Some(Action::PromptCancel),
            KeyCode::Char(c) => Some(Action::PromptPush(c)),
            KeyCode::Backspace => Some(Action::PromptPop),
            KeyCode::Enter => Some(Action::PromptSubmit),
            KeyCode::Esc => Some(Action::PromptCancel),
            _ => None,
        };
    }

    match key.code {
        KeyCode::Char('d') if ctrl => Some(Action::HalfDown),
        KeyCode::Char('u') if ctrl => Some(Action::HalfUp),
        KeyCode::Char('c') if ctrl => Some(Action::Quit),

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
        action_for(key(code), Mode::Record, false)
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
            action_for(ctrl('d'), Mode::Record, false),
            Some(Action::HalfDown)
        );
        assert_eq!(
            action_for(ctrl('u'), Mode::Record, false),
            Some(Action::HalfUp)
        );
    }

    #[test]
    fn plain_letters_are_not_confused_with_control_chords() {
        assert_eq!(record(KeyCode::Char('d')), None);
        assert_eq!(record(KeyCode::Char('u')), None);
    }

    #[test]
    fn quit_only_exits_from_the_record_view() {
        assert_eq!(record(KeyCode::Char('q')), Some(Action::Quit));
        assert_eq!(
            action_for(key(KeyCode::Char('q')), Mode::Pager, false),
            Some(Action::Back),
            "q in the pager steps back rather than exiting"
        );
        assert_eq!(
            action_for(key(KeyCode::Char('q')), Mode::Table, false),
            Some(Action::Back)
        );
    }

    #[test]
    fn control_c_always_quits() {
        assert_eq!(
            action_for(ctrl('c'), Mode::Pager, false),
            Some(Action::Quit)
        );
    }

    #[test]
    fn capitals_scroll_columns() {
        assert_eq!(record(KeyCode::Char('H')), Some(Action::ColumnLeft));
        assert_eq!(record(KeyCode::Char('L')), Some(Action::ColumnRight));
    }

    #[test]
    fn prompts_capture_text() {
        assert_eq!(
            action_for(key(KeyCode::Char('j')), Mode::Record, true),
            Some(Action::PromptPush('j')),
            "letters are text, not navigation, while prompting"
        );
        assert_eq!(
            action_for(key(KeyCode::Enter), Mode::Record, true),
            Some(Action::PromptSubmit)
        );
        assert_eq!(
            action_for(key(KeyCode::Backspace), Mode::Record, true),
            Some(Action::PromptPop)
        );
        assert_eq!(
            action_for(key(KeyCode::Esc), Mode::Record, true),
            Some(Action::PromptCancel)
        );
    }

    #[test]
    fn prompt_captures_keys_that_are_commands_otherwise() {
        for c in ['q', '/', ':', 'z', 'w', '?'] {
            assert_eq!(
                action_for(key(KeyCode::Char(c)), Mode::Record, true),
                Some(Action::PromptPush(c)),
                "{c} must be typeable in a prompt"
            );
        }
    }

    #[test]
    fn control_c_escapes_a_prompt() {
        assert_eq!(
            action_for(ctrl('c'), Mode::Record, true),
            Some(Action::PromptCancel)
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
        assert_eq!(action_for(event, Mode::Record, false), None);
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
