//! Text layout: normalisation, wrapping, clamping and record-body assembly.
//!
//! Everything here is pure. Nothing reads the terminal — widths and heights
//! arrive as arguments — which is what makes the awkward cases (CJK, emoji,
//! one-column terminals, fields taller than the screen) cheap to test.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Spaces a tab expands to for display.
const TAB_WIDTH: usize = 4;
/// Smallest field cap, so a short terminal still shows something useful.
pub const MIN_FIELD_HEIGHT: usize = 3;

/// What a field contains, once you look past the whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Empty,
    Whitespace,
    Text,
}

impl FieldKind {
    /// Placeholder shown for fields with nothing readable in them.
    pub fn placeholder(self) -> Option<&'static str> {
        match self {
            Self::Empty => Some("(empty)"),
            Self::Whitespace => Some("(whitespace)"),
            Self::Text => None,
        }
    }
}

/// A field's text, wrapped to a specific width.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldLayout {
    pub kind: FieldKind,
    pub lines: Vec<String>,
}

/// Display width in terminal cells.
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// Normalise line endings and tabs for display.
///
/// Yank deliberately does not use this — the clipboard gets the raw text.
pub fn normalise(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\t' => out.push_str(&" ".repeat(TAB_WIDTH)),
            _ => out.push(ch),
        }
    }
    out
}

/// Classify a field by what it would show.
pub fn classify(raw: &str) -> FieldKind {
    if raw.is_empty() {
        FieldKind::Empty
    } else if raw.trim().is_empty() {
        FieldKind::Whitespace
    } else {
        FieldKind::Text
    }
}

/// Cut a string to `width` cells, appending `…` when anything was dropped.
pub fn truncate_to_width(s: &str, width: usize) -> String {
    if display_width(s) <= width {
        return s.to_owned();
    }
    match width {
        0 => String::new(),
        1 => "…".to_owned(),
        _ => {
            let budget = width - 1;
            let mut out = String::new();
            let mut used = 0;
            for ch in s.chars() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(0);
                if used + w > budget {
                    break;
                }
                out.push(ch);
                used += w;
            }
            out.push('…');
            out
        }
    }
}

/// Drop the first `cells` columns of a string, for horizontal scrolling.
pub fn skip_width(s: &str, cells: usize) -> String {
    if cells == 0 {
        return s.to_owned();
    }
    let mut used = 0;
    let mut out = String::new();
    for ch in s.chars() {
        if used >= cells {
            out.push(ch);
        } else {
            used += UnicodeWidthChar::width(ch).unwrap_or(0);
        }
    }
    out
}

/// Collapse a field to a single line for table cells.
pub fn one_line(raw: &str, width: usize) -> String {
    let kind = classify(raw);
    if let Some(placeholder) = kind.placeholder() {
        return truncate_to_width(placeholder, width);
    }
    let flattened: String = normalise(raw)
        .lines()
        .collect::<Vec<_>>()
        .join(" \u{23ce} ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_to_width(&flattened, width)
}

/// Wrap or chop a field's text to `width`.
pub fn layout_field(raw: &str, width: usize, wrap: bool) -> FieldLayout {
    let kind = classify(raw);
    if let Some(placeholder) = kind.placeholder() {
        return FieldLayout {
            kind,
            lines: vec![placeholder.to_owned()],
        };
    }

    let width = width.max(1);
    let text = normalise(raw);
    let mut lines = Vec::new();
    for logical in text.split('\n') {
        if logical.is_empty() {
            lines.push(String::new());
        } else if wrap {
            for piece in textwrap::wrap(logical, width) {
                lines.push(piece.into_owned());
            }
        } else {
            lines.push(truncate_to_width(logical, width));
        }
    }
    FieldLayout { kind, lines }
}

/// Lay a field out for the pager, honouring a horizontal offset.
///
/// A non-zero offset chops rather than wraps, matching how `less` switches to
/// `-S` mode as soon as you scroll sideways.
pub fn pager_lines(raw: &str, width: usize, h_offset: usize, wrap: bool) -> Vec<String> {
    if h_offset == 0 && wrap {
        return layout_field(raw, width, true).lines;
    }
    let kind = classify(raw);
    if let Some(placeholder) = kind.placeholder() {
        return vec![placeholder.to_owned()];
    }
    normalise(raw)
        .split('\n')
        .map(|line| {
            let shifted = skip_width(line, h_offset);
            truncate_to_width(&shifted, width.max(1))
        })
        .collect()
}

/// The widest logical line in a field, used to bound horizontal scrolling.
pub fn longest_line(raw: &str) -> usize {
    normalise(raw)
        .split('\n')
        .map(display_width)
        .max()
        .unwrap_or(0)
}

/// What a single rendered line of the record body represents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineRole {
    /// Column name. Carries the counts the status suffix needs.
    Header {
        total: usize,
        shown: usize,
    },
    Content,
    /// Stand-in for a field with nothing readable in it.
    Placeholder,
    /// The `⋯ N more lines` marker closing a clamped field.
    More {
        hidden: usize,
    },
    /// Separator between fields.
    Blank,
}

/// One rendered line of the record body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyLine {
    /// Field this line belongs to; blanks carry the field they follow.
    pub field: usize,
    pub role: LineRole,
    pub text: String,
}

impl BodyLine {
    /// Whether the selection marker should appear against this line.
    pub fn selectable(&self) -> bool {
        !matches!(self.role, LineRole::Blank)
    }
}

/// Height cap for a field, as a percentage of the available body height.
pub fn field_cap(body_height: usize, percent: u8) -> usize {
    let scaled = body_height * usize::from(percent) / 100;
    scaled.max(MIN_FIELD_HEIGHT)
}

/// Assemble the full record body as a flat list of lines.
///
/// Flattening makes scrolling an index offset rather than a nest of per-field
/// offsets, which is what keeps the record view free of nested scroll state.
pub fn build_body(
    headers: &[String],
    row: &[String],
    width: usize,
    cap: usize,
    expanded: Option<usize>,
    wrap: bool,
) -> Vec<BodyLine> {
    let mut body = Vec::new();
    for (index, name) in headers.iter().enumerate() {
        let cell = row.get(index).map_or("", String::as_str);
        let layout = layout_field(cell, width, wrap);
        let total = layout.lines.len();
        let limit = if expanded == Some(index) { total } else { cap };
        let shown = total.min(limit);

        if index > 0 {
            body.push(BodyLine {
                field: index - 1,
                role: LineRole::Blank,
                text: String::new(),
            });
        }
        body.push(BodyLine {
            field: index,
            role: LineRole::Header { total, shown },
            text: name.clone(),
        });
        let role = if layout.kind == FieldKind::Text {
            LineRole::Content
        } else {
            LineRole::Placeholder
        };
        for line in layout.lines.into_iter().take(shown) {
            body.push(BodyLine {
                field: index,
                role: role.clone(),
                text: line,
            });
        }
        if total > shown {
            body.push(BodyLine {
                field: index,
                role: LineRole::More {
                    hidden: total - shown,
                },
                text: String::new(),
            });
        }
    }
    body
}

/// Index range of a field's lines within the body, excluding leading blanks.
pub fn field_span(body: &[BodyLine], field: usize) -> Option<(usize, usize)> {
    let start = body
        .iter()
        .position(|l| l.field == field && l.selectable())?;
    let end = body
        .iter()
        .rposition(|l| l.field == field && l.selectable())?;
    Some((start, end))
}

/// Scroll offset that brings a field into view with the least movement.
pub fn scroll_to_show(body: &[BodyLine], field: usize, scroll: usize, height: usize) -> usize {
    let Some((start, end)) = field_span(body, field) else {
        return scroll;
    };
    let height = height.max(1);
    let max_scroll = body.len().saturating_sub(height);

    let scroll = if start < scroll {
        start
    } else if end >= scroll + height {
        // Prefer showing the top of a field that is taller than the viewport.
        let needed = end + 1 - height;
        needed.min(start)
    } else {
        scroll
    };
    scroll.min(max_scroll)
}

/// Clamp a scroll offset to the content, so the view never runs past the end.
pub fn clamp_scroll(scroll: usize, len: usize, height: usize) -> usize {
    scroll.min(len.saturating_sub(height.max(1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_line_endings_and_tabs() {
        assert_eq!(normalise("a\r\nb\rc"), "a\nb\nc");
        assert_eq!(normalise("a\tb"), "a    b");
    }

    #[test]
    fn classifies_fields() {
        assert_eq!(classify(""), FieldKind::Empty);
        assert_eq!(classify("   "), FieldKind::Whitespace);
        assert_eq!(classify("\n\t"), FieldKind::Whitespace);
        assert_eq!(classify("x"), FieldKind::Text);
    }

    #[test]
    fn placeholders_distinguish_empty_from_whitespace() {
        assert_eq!(layout_field("", 20, true).lines, ["(empty)"]);
        assert_eq!(layout_field("   ", 20, true).lines, ["(whitespace)"]);
    }

    #[test]
    fn truncation_is_width_aware() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
        assert_eq!(truncate_to_width("hello", 3), "he…");
        assert_eq!(truncate_to_width("hello", 1), "…");
        assert_eq!(truncate_to_width("hello", 0), "");
    }

    #[test]
    fn truncation_never_splits_a_wide_character() {
        // Each CJK glyph is two cells, so a 3-cell budget fits one plus "…".
        let out = truncate_to_width("日本語です", 3);
        assert_eq!(out, "日…");
        assert!(display_width(&out) <= 3);
    }

    #[test]
    fn skips_columns_for_horizontal_scroll() {
        assert_eq!(skip_width("abcdef", 2), "cdef");
        assert_eq!(skip_width("abc", 0), "abc");
        assert_eq!(skip_width("abc", 99), "");
    }

    #[test]
    fn one_line_marks_embedded_newlines() {
        assert_eq!(one_line("a\nb", 40), "a \u{23ce} b");
        assert_eq!(one_line("", 40), "(empty)");
    }

    #[test]
    fn wrapping_preserves_blank_lines_between_paragraphs() {
        let out = layout_field("one\n\ntwo", 40, true);
        assert_eq!(out.lines, ["one", "", "two"]);
    }

    #[test]
    fn wrapping_splits_long_lines() {
        let out = layout_field("aaa bbb ccc", 7, true);
        assert_eq!(out.lines, ["aaa bbb", "ccc"]);
    }

    #[test]
    fn truncate_mode_keeps_one_line_per_logical_line() {
        let out = layout_field("aaa bbb ccc", 7, false);
        assert_eq!(out.lines, ["aaa bb…"]);
    }

    #[test]
    fn zero_width_does_not_panic() {
        let out = layout_field("hello world", 0, true);
        assert!(!out.lines.is_empty());
    }

    #[test]
    fn cap_respects_the_floor() {
        assert_eq!(field_cap(100, 40), 40);
        assert_eq!(field_cap(4, 40), MIN_FIELD_HEIGHT);
        assert_eq!(field_cap(0, 40), MIN_FIELD_HEIGHT);
    }

    fn sample_body(cap: usize, expanded: Option<usize>) -> Vec<BodyLine> {
        let headers = vec!["a".to_owned(), "b".to_owned()];
        let row = vec!["one\ntwo\nthree\nfour".to_owned(), "short".to_owned()];
        build_body(&headers, &row, 40, cap, expanded, true)
    }

    #[test]
    fn body_clamps_tall_fields_and_marks_the_remainder() {
        let body = sample_body(2, None);
        let more = body
            .iter()
            .find(|l| matches!(l.role, LineRole::More { .. }))
            .expect("expected a more marker");
        assert_eq!(more.role, LineRole::More { hidden: 2 });
        assert_eq!(more.field, 0);
    }

    #[test]
    fn body_header_reports_totals() {
        let body = sample_body(2, None);
        assert_eq!(body[0].role, LineRole::Header { total: 4, shown: 2 });
    }

    #[test]
    fn expanding_a_field_lifts_the_cap() {
        let body = sample_body(2, Some(0));
        assert!(!body.iter().any(|l| matches!(l.role, LineRole::More { .. })));
        assert_eq!(body[0].role, LineRole::Header { total: 4, shown: 4 });
    }

    #[test]
    fn short_fields_render_at_natural_height() {
        let body = sample_body(10, None);
        let content = body
            .iter()
            .filter(|l| l.field == 1 && l.role == LineRole::Content)
            .count();
        assert_eq!(content, 1);
    }

    #[test]
    fn field_span_covers_header_and_content() {
        let body = sample_body(10, None);
        let (start, end) = field_span(&body, 1).expect("field 1 exists");
        assert_eq!(body[start].role, LineRole::Header { total: 1, shown: 1 });
        assert_eq!(body[end].role, LineRole::Content);
    }

    #[test]
    fn scrolling_reveals_a_field_below_the_fold() {
        let body = sample_body(10, None);
        let (start, _) = field_span(&body, 1).expect("field 1 exists");
        let scroll = scroll_to_show(&body, 1, 0, 3);
        assert!(scroll <= start);
        assert!(scroll > 0, "should have scrolled down to reach field 1");
    }

    #[test]
    fn scrolling_prefers_the_top_of_an_oversized_field() {
        let body = sample_body(10, None);
        let (start, _) = field_span(&body, 0).expect("field 0 exists");
        assert_eq!(scroll_to_show(&body, 0, 0, 2), start);
    }

    #[test]
    fn scrolling_leaves_a_visible_field_alone() {
        let body = sample_body(10, None);
        assert_eq!(scroll_to_show(&body, 0, 0, 20), 0);
    }

    #[test]
    fn pager_chops_once_shifted() {
        // Shifting by 3 leaves "defghij", which still overflows 5 cells and so
        // keeps the same truncation marker used everywhere else.
        assert_eq!(pager_lines("abcdefghij", 5, 3, true), ["defg…"]);
        assert_eq!(pager_lines("abcdefghij", 9, 3, true), ["defghij"]);
    }

    #[test]
    fn pager_wraps_when_not_shifted() {
        assert_eq!(pager_lines("aaa bbb ccc", 7, 0, true), ["aaa bbb", "ccc"]);
    }

    #[test]
    fn pager_chops_when_wrap_is_off() {
        assert_eq!(pager_lines("aaa bbb ccc", 7, 0, false), ["aaa bb…"]);
    }

    #[test]
    fn longest_line_measures_the_widest() {
        assert_eq!(longest_line("ab\nabcd\na"), 4);
    }

    #[test]
    fn clamping_keeps_the_view_on_content() {
        assert_eq!(clamp_scroll(99, 10, 4), 6);
        assert_eq!(clamp_scroll(2, 10, 4), 2);
        assert_eq!(clamp_scroll(5, 2, 4), 0);
    }
}
