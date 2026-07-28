//! The field editor's buffer: text, a caret, and the pure operations on both.
//!
//! The caret is a byte offset into the field's raw text, never a screen
//! coordinate. Everything that needs a screen coordinate goes through [`rows`],
//! which takes the width as an argument the way `layout` does, so none of this
//! knows a terminal exists.
//!
//! The wrap here is deliberately *not* `layout::layout_field`. Wrapping for
//! display may drop the whitespace it breaks on; wrapping for editing may not,
//! because a caret offset has to map to exactly one screen position and back.
//! [`rows`] therefore partitions the text — every byte belongs to exactly one
//! row — which is what makes the mapping invertible.

use unicode_width::UnicodeWidthChar;

use crate::layout::{TAB_WIDTH, normalise_newlines};

/// A field's raw text with a caret in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Buffer {
    pub text: String,
    /// Caret position as a byte offset. Always on a character boundary.
    pub caret: usize,
}

/// One visual row: the half-open byte range of the text it displays.
///
/// A row's `end` excludes the newline that terminated it, so `end` is where the
/// caret sits at the end of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    pub start: usize,
    pub end: usize,
}

/// How the editor is drawn.
///
/// One buffer, two renderings. Most fields are short — an id, a date, a price
/// — and swapping the whole screen to change four characters loses the record
/// you were looking at. Reading a long field is the case the pager already
/// exists for, so an edit started there stays where it started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// In place among the other fields, in the record view.
    Inline,
    /// Filling the screen, as the pager does.
    FullScreen,
}

/// A field being edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editing {
    /// Row and column captured on entry, so nothing can retarget the commit
    /// part-way through.
    pub row: usize,
    pub field: usize,
    pub buffer: Buffer,
    /// The text as it was on entry, for cancelling and for detecting changes.
    pub original: String,
    /// First visible row of the buffer. Only the full-screen surface uses
    /// this; inline, the record body has one scroll offset for everything.
    pub scroll: usize,
    /// Set once `Esc` has asked whether to discard unsaved changes.
    pub confirming: bool,
    pub surface: Surface,
}

impl Editing {
    /// Open a field for editing.
    ///
    /// Line endings are normalised on entry: the caret has to count lines
    /// against one convention, so a field mixing `\r\n` and `\n` would
    /// otherwise put the caret in a different place than the screen shows.
    /// Tabs are left as they are — expanding those would rewrite the file.
    ///
    /// The caret starts at the top, so the editor opens on the same part of
    /// the field the viewer was already showing.
    pub fn new(row: usize, field: usize, raw: &str, surface: Surface) -> Self {
        let text = normalise_newlines(raw);
        Self {
            row,
            field,
            buffer: Buffer {
                text: text.clone(),
                caret: 0,
            },
            original: text,
            scroll: 0,
            confirming: false,
            surface,
        }
    }

    /// The buffer's text laid out for the screen, one string per visual row.
    ///
    /// The record body needs these as plain lines; the caret maps through the
    /// same partition, which is what keeps the two in step.
    pub fn lines(&self, width: usize) -> Vec<String> {
        rows(&self.buffer.text, width)
            .into_iter()
            .map(|row| expand_tabs(&self.buffer.text[row.start..row.end]))
            .collect()
    }

    /// Whether the buffer differs from what was opened.
    pub fn modified(&self) -> bool {
        self.buffer.text != self.original
    }
}

/// Cells one character occupies, counting a tab as a fixed-width indent.
///
/// Real tab stops depend on the column, but `layout::normalise` already
/// expands a tab to exactly [`TAB_WIDTH`] spaces everywhere else in the
/// viewer, and the editor has to agree with the renderer rather than with
/// any particular terminal.
fn char_cells(ch: char) -> usize {
    if ch == '\t' {
        TAB_WIDTH
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

/// Display width of a string, counting tabs as [`TAB_WIDTH`].
pub fn cells(text: &str) -> usize {
    text.chars().map(char_cells).sum()
}

/// Render a row's text, expanding tabs so the terminal cannot disagree about
/// how wide they are.
pub fn expand_tabs(text: &str) -> String {
    text.replace('\t', &" ".repeat(TAB_WIDTH))
}

/// Partition the text into visual rows of at most `width` cells.
///
/// Breaks after the last space that fits, so words stay whole, but the space
/// stays on the row it ended — no byte is ever dropped. A character wider than
/// the whole width still gets a row to itself rather than looping for ever.
pub fn rows(text: &str, width: usize) -> Vec<Row> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        let end = start + line.len();
        wrap_line(text, start, end, width, &mut out);
        // Step over the newline that split() consumed.
        start = end + 1;
    }
    out
}

/// Append the rows of one logical line, which is at least one row even when
/// the line is empty.
fn wrap_line(text: &str, lo: usize, hi: usize, width: usize, out: &mut Vec<Row>) {
    let mut start = lo;
    loop {
        let mut used = 0;
        let mut last_space = None;
        let mut overflow = None;

        for (offset, ch) in text[start..hi].char_indices() {
            let cells = char_cells(ch);
            if used + cells > width {
                overflow = Some(start + offset);
                break;
            }
            used += cells;
            if ch == ' ' {
                last_space = Some(start + offset + ch.len_utf8());
            }
        }

        let Some(overflow) = overflow else {
            out.push(Row { start, end: hi });
            return;
        };

        let end = match last_space {
            Some(space) if space > start && space <= overflow => space,
            // Nothing to break on, so break mid-word — and never at `start`
            // itself, which would make no progress.
            _ => overflow.max(after_first_char(text, start, hi)),
        };
        out.push(Row { start, end });
        start = end;
        // Breaking exactly at the end of the line finishes it. Looping once
        // more would add an empty row the line does not have.
        if start >= hi {
            return;
        }
    }
}

/// Offset just past the character at `at`, bounded by `hi`.
fn after_first_char(text: &str, at: usize, hi: usize) -> usize {
    text[at..hi]
        .chars()
        .next()
        .map_or(hi, |ch| at + ch.len_utf8())
}

/// Which row a caret offset falls on.
///
/// A caret sitting exactly on a wrap point belongs to the row it starts, which
/// is where the cursor appears after typing the character that caused the wrap.
pub fn row_at(rows: &[Row], caret: usize) -> usize {
    rows.iter().rposition(|row| row.start <= caret).unwrap_or(0)
}

/// Cell column of a caret within its row.
pub fn column_at(text: &str, row: Row, caret: usize) -> usize {
    cells(&text[row.start..caret.clamp(row.start, row.end)])
}

/// Offset within a row that sits at or just before `column`.
fn offset_for_column(text: &str, row: Row, column: usize) -> usize {
    let mut used = 0;
    for (offset, ch) in text[row.start..row.end].char_indices() {
        if used >= column {
            return row.start + offset;
        }
        used += char_cells(ch);
    }
    row.end
}

/// Caret position as a (row, column) screen coordinate.
pub fn caret_position(buffer: &Buffer, width: usize) -> (usize, usize) {
    let rows = rows(&buffer.text, width);
    let index = row_at(&rows, buffer.caret);
    let column = rows
        .get(index)
        .map_or(0, |row| column_at(&buffer.text, *row, buffer.caret));
    (index, column)
}

/// Insert a character at the caret.
pub fn insert(buffer: &Buffer, ch: char) -> Buffer {
    let mut text = buffer.text.clone();
    text.insert(buffer.caret, ch);
    Buffer {
        text,
        caret: buffer.caret + ch.len_utf8(),
    }
}

/// Delete the character before the caret.
pub fn backspace(buffer: &Buffer) -> Buffer {
    let Some(previous) = previous_boundary(&buffer.text, buffer.caret) else {
        return buffer.clone();
    };
    let mut text = buffer.text.clone();
    text.replace_range(previous..buffer.caret, "");
    Buffer {
        text,
        caret: previous,
    }
}

/// Delete the character under the caret.
pub fn delete(buffer: &Buffer) -> Buffer {
    let Some(next) = next_boundary(&buffer.text, buffer.caret) else {
        return buffer.clone();
    };
    let mut text = buffer.text.clone();
    text.replace_range(buffer.caret..next, "");
    Buffer {
        text,
        caret: buffer.caret,
    }
}

/// Move the caret one character left.
pub fn left(buffer: &Buffer) -> Buffer {
    moved(buffer, previous_boundary(&buffer.text, buffer.caret))
}

/// Move the caret one character right.
pub fn right(buffer: &Buffer) -> Buffer {
    moved(buffer, next_boundary(&buffer.text, buffer.caret))
}

/// Move the caret `delta` visual rows, keeping its column where it can.
pub fn move_rows(buffer: &Buffer, width: usize, delta: isize) -> Buffer {
    let rows = rows(&buffer.text, width);
    let index = row_at(&rows, buffer.caret);
    let Some(current) = rows.get(index) else {
        return buffer.clone();
    };
    let column = column_at(&buffer.text, *current, buffer.caret);
    let target = index
        .saturating_add_signed(delta)
        .min(rows.len().saturating_sub(1));
    moved(
        buffer,
        rows.get(target)
            .map(|row| offset_for_column(&buffer.text, *row, column)),
    )
}

/// Move the caret to the start of its visual row.
pub fn row_start(buffer: &Buffer, width: usize) -> Buffer {
    let rows = rows(&buffer.text, width);
    let index = row_at(&rows, buffer.caret);
    moved(buffer, rows.get(index).map(|row| row.start))
}

/// Move the caret to the end of its visual row.
pub fn row_end(buffer: &Buffer, width: usize) -> Buffer {
    let rows = rows(&buffer.text, width);
    let index = row_at(&rows, buffer.caret);
    moved(buffer, rows.get(index).map(|row| row.end))
}

fn moved(buffer: &Buffer, caret: Option<usize>) -> Buffer {
    Buffer {
        text: buffer.text.clone(),
        caret: caret.unwrap_or(buffer.caret),
    }
}

fn previous_boundary(text: &str, at: usize) -> Option<usize> {
    text.get(..at)?
        .chars()
        .next_back()
        .map(|ch| at - ch.len_utf8())
}

fn next_boundary(text: &str, at: usize) -> Option<usize> {
    text.get(at..)?.chars().next().map(|ch| at + ch.len_utf8())
}

#[cfg(test)]
#[path = "edit_tests.rs"]
mod tests;
