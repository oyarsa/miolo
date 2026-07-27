//! Markdown fence detection for the pager.
//!
//! Deliberately not a syntax highlighter. Fields are plain text unless they
//! literally contain fences, in which case the fenced region is tinted and the
//! markers dimmed. The language tag is displayed but not acted upon.

/// What a display line is, as far as fence handling is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Segment {
    /// Ordinary prose.
    Text,
    /// A fence line opening or closing a block.
    Fence,
    /// A line inside a fenced block.
    Code,
}

/// The fence marker.
const FENCE: &str = "```";

/// Classify each display line by walking the fence state machine.
///
/// Operating on display rather than logical lines is safe because a fence
/// marker is only three characters and so never wraps.
pub fn classify(lines: &[String]) -> Vec<Segment> {
    let mut inside = false;
    lines
        .iter()
        .map(|line| {
            if line.trim_start().starts_with(FENCE) {
                inside = !inside;
                Segment::Fence
            } else if inside {
                Segment::Code
            } else {
                Segment::Text
            }
        })
        .collect()
}

/// Whether a field is worth running fence handling over at all.
pub fn has_fence(raw: &str) -> bool {
    raw.lines().any(|l| l.trim_start().starts_with(FENCE))
}

/// Whether a field looks like a structured value rather than prose.
///
/// Detected from the text rather than recorded at load time, so no per-cell
/// type has to be carried through the table. A CSV cell that happens to hold
/// JSON is tinted too, which is a feature rather than a cost.
pub fn looks_structured(raw: &str) -> bool {
    let trimmed = raw.trim_start();
    (trimmed.starts_with('{') || trimmed.starts_with('[')) && raw.contains('\n')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(str::to_owned).collect()
    }

    #[test]
    fn plain_text_is_all_text() {
        let out = classify(&lines("one\ntwo"));
        assert_eq!(out, [Segment::Text, Segment::Text]);
    }

    #[test]
    fn fenced_block_is_marked() {
        let out = classify(&lines("intro\n```json\n{}\n```\nafter"));
        assert_eq!(
            out,
            [
                Segment::Text,
                Segment::Fence,
                Segment::Code,
                Segment::Fence,
                Segment::Text
            ]
        );
    }

    #[test]
    fn indented_fences_still_count() {
        let out = classify(&lines("  ```\n  x\n  ```"));
        assert_eq!(out, [Segment::Fence, Segment::Code, Segment::Fence]);
    }

    #[test]
    fn unclosed_fence_runs_to_the_end() {
        let out = classify(&lines("```\na\nb"));
        assert_eq!(out, [Segment::Fence, Segment::Code, Segment::Code]);
    }

    #[test]
    fn structured_values_are_recognised() {
        assert!(looks_structured("{\n  \"a\": 1\n}"));
        assert!(looks_structured("[\n  1,\n  2\n]"));
        assert!(looks_structured("  {\n\"a\": 1}"), "leading space is fine");
    }

    #[test]
    fn prose_and_one_liners_are_not_structured() {
        assert!(!looks_structured("just some prose\nover two lines"));
        assert!(!looks_structured("{\"a\": 1}"), "single line stays plain");
        assert!(!looks_structured(""));
    }

    #[test]
    fn detects_whether_a_field_has_any_fence() {
        assert!(has_fence("a\n```\nb"));
        assert!(!has_fence("a\nb"));
        assert!(!has_fence("a ``` inline"), "must start the line");
    }
}
