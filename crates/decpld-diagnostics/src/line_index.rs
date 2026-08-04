//! Byte offset → line/column, for rendering diagnostics against source.

/// A 1-based line and column, as a human reads them in an editor.
///
/// `column` counts **characters**, not bytes: reporting a byte offset
/// would put the caret in the wrong place on any line containing
/// non-ASCII, which JEDEC note fields and deCPLD comments may.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineCol {
    pub line: u32,
    pub column: u32,
}

/// A multi-byte character's position and how much wider than one byte it
/// is. Only non-ASCII characters are recorded, so ASCII source costs
/// nothing beyond the line table.
#[derive(Clone, Copy, Debug)]
struct MultiByteChar {
    offset: u32,
    extra_bytes: u32,
}

/// Byte-offset → line/column lookup for one source file.
///
/// Built once per file and reused; the text is not retained, so a
/// `LineIndex` can outlive the buffer it was built from.
#[derive(Clone, Debug)]
pub struct LineIndex {
    /// Byte offset of each line start. Always begins with 0, so there is
    /// always at least one line even in empty source.
    line_starts: Vec<u32>,
    multibyte: Vec<MultiByteChar>,
    len: u32,
}

impl LineIndex {
    #[must_use]
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        let mut multibyte = Vec::new();

        for (offset, ch) in text.char_indices() {
            let offset = offset as u32;
            if ch == '\n' {
                line_starts.push(offset + 1);
            } else if ch.len_utf8() > 1 {
                multibyte.push(MultiByteChar { offset, extra_bytes: ch.len_utf8() as u32 - 1 });
            }
        }

        Self { line_starts, multibyte, len: text.len() as u32 }
    }

    /// The 1-based line and character column of `offset`.
    ///
    /// An offset past the end clamps to the last position rather than
    /// panicking: "unexpected end of input" diagnostics point one past
    /// the end, and the error path is the worst place to panic.
    #[must_use]
    pub fn line_col(&self, offset: u32) -> LineCol {
        let offset = offset.min(self.len);
        // `partition_point` gives the count of line starts at or before
        // `offset`; that count minus one is the 0-based line.
        let line_index = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let line_start = self.line_starts[line_index];

        // Each multi-byte character between the line start and `offset`
        // inflates the byte column by `extra_bytes`; subtract that back
        // out to get a character column.
        let extra: u32 = self
            .multibyte
            .iter()
            .filter(|mb| mb.offset >= line_start && mb.offset < offset)
            .map(|mb| mb.extra_bytes)
            .sum();

        LineCol { line: line_index as u32 + 1, column: offset - line_start - extra + 1 }
    }

    /// The number of lines. Source ending in a newline has a final empty
    /// line, matching how editors number them.
    #[must_use]
    pub fn line_count(&self) -> u32 {
        self.line_starts.len() as u32
    }

    /// The text of a 1-based line, without its terminator.
    ///
    /// Takes the source rather than retaining it, so the index stays
    /// cheap to store alongside long-lived structures.
    #[must_use]
    pub fn line_text<'t>(&self, text: &'t str, line: u32) -> Option<&'t str> {
        if line == 0 || line > self.line_count() {
            return None;
        }
        let index = (line - 1) as usize;
        let start = self.line_starts[index] as usize;
        // Every line start after the first is one past a '\n', so the
        // line ends at that newline; the last line ends at the text end.
        let end = match self.line_starts.get(index + 1) {
            Some(&next) => (next - 1) as usize,
            None => self.len as usize,
        };
        // CRLF: the '\r' terminates the line and is not part of it.
        Some(text[start..end].strip_suffix('\r').unwrap_or(&text[start..end]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_starts_at_one_one() {
        let index = LineIndex::new("QF500*\nF0*\n");
        assert_eq!(index.line_col(0), LineCol { line: 1, column: 1 });
    }

    #[test]
    fn columns_advance_within_a_line() {
        let index = LineIndex::new("QF500*\nF0*\n");
        assert_eq!(index.line_col(4), LineCol { line: 1, column: 5 });
    }

    #[test]
    fn offsets_after_a_newline_are_on_the_next_line() {
        let index = LineIndex::new("QF500*\nF0*\n");
        assert_eq!(index.line_col(7), LineCol { line: 2, column: 1 });
        assert_eq!(index.line_col(9), LineCol { line: 2, column: 3 });
    }

    #[test]
    fn crlf_does_not_produce_a_phantom_column() {
        // JEDEC files routinely use CRLF (they came from DOS tooling).
        // The `\r` belongs to the line it terminates, so the first
        // character after `\r\n` must be column 1 of the next line —
        // not column 2 of a line that appears to start with a stray CR.
        let index = LineIndex::new("QF500*\r\nF0*\r\n");
        assert_eq!(index.line_col(8), LineCol { line: 2, column: 1 });
    }

    #[test]
    fn an_offset_past_the_end_clamps_to_the_last_position() {
        // Diagnostics for "unexpected end of input" point one past the
        // end. Clamping keeps that renderable instead of panicking in
        // the error path, which is the worst place to panic.
        let index = LineIndex::new("QF500*");
        assert_eq!(index.line_col(999), LineCol { line: 1, column: 7 });
    }

    #[test]
    fn empty_source_is_line_one_column_one() {
        let index = LineIndex::new("");
        assert_eq!(index.line_col(0), LineCol { line: 1, column: 1 });
    }

    #[test]
    fn columns_count_characters_not_bytes() {
        // A note field may contain non-ASCII. Reporting a byte offset as
        // a column would put the caret in the wrong place in any editor.
        let index = LineIndex::new("N café*\nQF12*");
        let offset = "N café".len(); // 7 bytes, 6 characters
        assert_eq!(index.line_col(offset as u32), LineCol { line: 1, column: 7 });
    }

    #[test]
    fn reports_the_text_of_a_line() {
        let index = LineIndex::new("QF500*\r\nF0*\n");
        assert_eq!(index.line_text("QF500*\r\nF0*\n", 1), Some("QF500*"));
        assert_eq!(index.line_text("QF500*\r\nF0*\n", 2), Some("F0*"));
        assert_eq!(index.line_text("QF500*\r\nF0*\n", 9), None);
    }
}
