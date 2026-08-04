//! Rendering diagnostics for a terminal.
//!
//! Kept out of the command functions so it can be tested without
//! spawning a process (CLAUDE.md → pragmatic layer). The language server
//! will render the same [`Diagnostic`] values differently; that both
//! consume one type is what keeps `decpld check` and an editor's
//! squiggle from ever disagreeing.

use decpld_diagnostics::{Diagnostic, DiagnosticBundle, LineIndex};
use std::fmt::Write as _;

/// Render one diagnostic as `path:line:col: error[E3001]: message`,
/// followed by its labels and notes.
#[must_use]
pub fn diagnostic(diagnostic: &Diagnostic, path: &str, source: &str, index: &LineIndex) -> String {
    let mut out = String::new();

    match diagnostic.primary_span() {
        Some(span) => {
            let at = index.line_col(span.range.start);
            let _ = writeln!(out, "{path}:{}:{}: {}", at.line, at.column, diagnostic.headline());
            // Show the offending line with a caret under it. Reading the
            // source beats describing it: an offset means nothing to a
            // human staring at a 70-line fuse map.
            if let Some(text) = index.line_text(source, at.line) {
                let _ = writeln!(out, "  {text}");
                let width = span.range.len().max(1) as usize;
                let _ = writeln!(
                    out,
                    "  {}{}",
                    " ".repeat(at.column.saturating_sub(1) as usize),
                    "^".repeat(width)
                );
            }
        }
        None => {
            let _ = writeln!(out, "{path}: {}", diagnostic.headline());
        }
    }

    for label in &diagnostic.labels {
        if !label.message.is_empty() {
            let at = index.line_col(label.span.range.start);
            let _ = writeln!(out, "  {}:{}: {}", at.line, at.column, label.message);
        }
    }
    for note in &diagnostic.notes {
        let _ = writeln!(out, "  note: {note}");
    }
    for fix in &diagnostic.fixes {
        let _ = writeln!(out, "  help: {}", fix.description);
    }

    out
}

/// Render every diagnostic in a bundle, in the order it was reported.
#[must_use]
pub fn bundle(bundle: &DiagnosticBundle, path: &str, source: &str) -> String {
    let index = LineIndex::new(source);
    bundle.iter().map(|d| diagnostic(d, path, source, &index)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use decpld_diagnostics::{DiagnosticCode, FileId, Label, Span, TextRange};

    fn index(source: &str) -> LineIndex {
        LineIndex::new(source)
    }

    #[test]
    fn a_located_diagnostic_names_file_line_and_column() {
        let source = "QF8*\nL0 1012*\n";
        let span = Span::new(FileId(0), TextRange::new(11, 12));
        let d = Diagnostic::error(DiagnosticCode::new(3012), "fuse state must be 0 or 1")
            .with_label(Label::primary(span, "not a fuse state"));

        let out = diagnostic(&d, "design.jed", source, &index(source));
        assert!(
            out.starts_with("design.jed:2:7: error[E3012]: fuse state must be 0 or 1"),
            "got:\n{out}"
        );
    }

    #[test]
    fn the_caret_sits_under_the_offending_character() {
        let source = "QF8*\nL0 1012*\n";
        let span = Span::new(FileId(0), TextRange::new(11, 12));
        let d = Diagnostic::error(DiagnosticCode::new(3012), "bad")
            .with_label(Label::primary(span, ""));

        let out = diagnostic(&d, "d.jed", source, &index(source));
        let lines: Vec<&str> = out.lines().collect();
        let text = lines[1];
        let carets = lines[2];
        // The caret column must line up with the `2` in `1012`.
        let caret_at = carets.find('^').expect("a caret");
        assert_eq!(text.as_bytes()[caret_at], b'2', "caret under {:?}", &text[caret_at..]);
    }

    #[test]
    fn a_diagnostic_without_a_span_still_names_the_file() {
        let source = "";
        let d = Diagnostic::error(DiagnosticCode::new(3001), "no STX character");
        let out = diagnostic(&d, "empty.jed", source, &index(source));
        assert_eq!(out.trim_end(), "empty.jed: error[E3001]: no STX character");
    }

    #[test]
    fn notes_and_helps_are_rendered_under_the_headline() {
        let source = "QF8*\n";
        let d = Diagnostic::error(DiagnosticCode::new(3015), "no QF field")
            .with_note("every JEDEC file must declare its fuse count");
        let out = diagnostic(&d, "d.jed", source, &index(source));
        assert!(out.contains("  note: every JEDEC file must declare its fuse count"), "{out}");
    }

    #[test]
    fn a_bundle_renders_in_report_order() {
        let source = "QF8*\n";
        let mut b = DiagnosticBundle::new();
        b.push(Diagnostic::error(DiagnosticCode::new(3001), "first"));
        b.push(Diagnostic::warning(DiagnosticCode::new(3040), "second"));

        let out = bundle(&b, "d.jed", source);
        let first = out.find("first").expect("first");
        let second = out.find("second").expect("second");
        assert!(first < second, "order must be preserved:\n{out}");
    }
}
