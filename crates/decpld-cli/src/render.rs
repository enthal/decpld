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
                // Control characters are data in a JEDEC file — STX, ETX
                // and embedded CRs all appear in line 1 — and echoing
                // them raw moves the terminal cursor, so the caret ends
                // up pointing at nothing.
                //
                // The stand-in is ASCII rather than U+FFFD because the
                // replacement character has *Ambiguous* East-Asian
                // width: terminals that draw it double-width shift every
                // following column by one, which is the exact damage the
                // substitution exists to prevent. `·` would read better
                // and has the same defect.
                let visible: String =
                    text.chars().map(|c| if c.is_control() { '?' } else { c }).collect();
                let _ = writeln!(out, "  {visible}");

                let leading = at.column.saturating_sub(1) as usize;
                // Width in CHARACTERS, not bytes: `span.range.len()` is a
                // byte count, and pairing it with a character column drew
                // two carets under a one-character `é`. Clamped to what
                // remains of the line so a span running past the end — an
                // unterminated field reaches the next line — cannot spray
                // carets into empty space.
                let width = source
                    .get(span.range.start as usize..span.range.end as usize)
                    .map_or(1, |s| s.chars().count())
                    .max(1)
                    .min(visible.chars().count().saturating_sub(leading).max(1));
                // The primary label's message rides on the caret, the way
                // rustc writes it. Its *location* is already the headline
                // and the caret, so repeating `line:col` below would be
                // noise — but dropping the message with it lost the only
                // explanation of what the caret is pointing at.
                let message = primary_message(diagnostic);
                let separator = if message.is_empty() { "" } else { " " };
                let _ = writeln!(
                    out,
                    "  {}{}{separator}{message}",
                    " ".repeat(leading),
                    "^".repeat(width)
                );
            }
        }
        None => {
            let _ = writeln!(out, "{path}: {}", diagnostic.headline());
        }
    }

    for label in &diagnostic.labels {
        // Already rendered on the caret line.
        if label.is_primary {
            continue;
        }
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

/// The primary label's message, or `""` if there is none.
///
/// Separated out because "which label is primary" is a decision, and a
/// decision inside a formatting expression is a decision nobody tests.
fn primary_message(diagnostic: &Diagnostic) -> &str {
    diagnostic
        .labels
        .iter()
        .find(|label| label.is_primary)
        .map_or("", |label| label.message.as_str())
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
    fn the_primary_labels_message_is_attached_to_the_caret() {
        // Found by the second review round. The primary message was
        // skipped entirely rather than relocated, so `E3002`'s "expected
        // ETX (0x03) before here" — the only thing that explains what a
        // caret at end-of-file means — appeared nowhere in the output.
        //
        // rustc, which this renderer imitates, puts the message on the
        // caret line. The redundancy the old comment objected to was the
        // repeated line:col, not the message.
        let source = "QF8*\nL0 1012*\n";
        let span = Span::new(FileId(0), TextRange::new(11, 12));
        let d = Diagnostic::error(DiagnosticCode::new(3012), "fuse state must be 0 or 1")
            .with_label(Label::primary(span, "not a fuse state"));

        let out = diagnostic(&d, "d.jed", source, &index(source));
        let caret_line = out.lines().nth(2).expect("a caret line");
        assert!(caret_line.contains('^'), "{out}");
        assert!(caret_line.ends_with("not a fuse state"), "got {caret_line:?} in:\n{out}");
        // And exactly once — not also repeated as a secondary line.
        assert_eq!(out.matches("not a fuse state").count(), 1, "{out}");
    }

    #[test]
    fn a_primary_label_with_no_message_leaves_the_caret_bare() {
        let source = "QF8*\n";
        let span = Span::new(FileId(0), TextRange::new(0, 1));
        let d = Diagnostic::error(DiagnosticCode::new(3012), "bad")
            .with_label(Label::primary(span, ""));
        let out = diagnostic(&d, "d.jed", source, &index(source));
        let caret_line = out.lines().nth(2).expect("a caret line");
        assert_eq!(caret_line.trim_end(), "  ^", "no trailing space: {caret_line:?}");
    }

    #[test]
    fn control_characters_are_shown_as_a_single_column_stand_in() {
        // STX and ETX are data in every JEDEC file, and echoing them raw
        // moves the terminal cursor so the caret points at nothing.
        //
        // The stand-in must be width-1 in every terminal: U+FFFD has
        // Ambiguous East-Asian width, so terminals that draw it
        // double-width shift the caret one column per substitution —
        // defeating the substitution's own purpose on line 1, which
        // contains two control characters.
        let source = "\x02h*QF8*\x03";
        let span = Span::new(FileId(0), TextRange::new(1, 2));
        let d = Diagnostic::error(DiagnosticCode::new(3012), "bad")
            .with_label(Label::primary(span, ""));
        let out = diagnostic(&d, "d.jed", source, &index(source));

        let shown = out.lines().nth(1).expect("the source line");
        assert!(!shown.contains('\u{2}'), "raw control character survived: {shown:?}");
        assert!(shown.is_ascii(), "the stand-in must be width-1 everywhere: {shown:?}");

        // The caret must still land on `h`, which follows the STX.
        let caret_at = out.lines().nth(2).expect("carets").find('^').expect("a caret");
        assert_eq!(shown.as_bytes()[caret_at], b'h', "caret under {:?}", &shown[caret_at..]);
    }

    #[test]
    fn a_span_running_past_the_end_of_its_line_does_not_spray_carets() {
        // An unterminated field's span reaches the following line. The
        // caret run is clamped to what is left of the line it is drawn
        // under, or the output is carets over empty space.
        let source = "QF8*\nL0 1010\nnext line\n";
        let span = Span::new(FileId(0), TextRange::new(5, 22));
        let d = Diagnostic::error(DiagnosticCode::new(3003), "field is missing its `*`")
            .with_label(Label::primary(span, ""));

        let out = diagnostic(&d, "d.jed", source, &index(source));
        let shown = out.lines().nth(1).expect("source line");
        let carets = out.lines().nth(2).expect("carets");
        assert!(
            carets.trim_end().len() <= shown.len(),
            "carets ran past the line:\n{shown:?}\n{carets:?}"
        );
    }

    #[test]
    fn the_caret_is_one_column_wide_for_a_one_character_multibyte_span() {
        // `span.range.len()` is a BYTE count; pairing it with a
        // character column drew two carets under a one-character `é`.
        let source = "N café*\n";
        let start = "N caf".len() as u32;
        let span = Span::new(FileId(0), TextRange::new(start, start + 2));
        let d = Diagnostic::warning(DiagnosticCode::new(3040), "odd")
            .with_label(Label::primary(span, ""));

        let out = diagnostic(&d, "d.jed", source, &index(source));
        let carets = out.lines().nth(2).expect("carets");
        assert_eq!(carets.matches('^').count(), 1, "one character, one caret: {carets:?}");
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
