//! Structured diagnostics. SPEC.md §5.18.

use std::fmt;

use crate::Span;
use crate::codes::Category;

/// How much a diagnostic matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
    /// Supporting information attached to another diagnostic.
    Note,
    /// A suggested action.
    Help,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Note => "note",
            Self::Help => "help",
        })
    }
}

/// A stable identifier for one kind of failure, rendered `E0204`.
///
/// See [`crate::codes`] for the ranges and the never-renumber rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DiagnosticCode(u16);

impl DiagnosticCode {
    #[must_use]
    pub const fn new(code: u16) -> Self {
        Self(code)
    }

    #[must_use]
    pub const fn as_u16(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn category(self) -> Category {
        Category::of(self.0)
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "E{:04}", self.0)
    }
}

/// A span with an explanation, pointing at the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub span: Span,
    pub message: String,
    /// Whether this is the location the diagnostic is *about*, as
    /// opposed to a contributing one.
    pub is_primary: bool,
}

impl Label {
    pub fn primary(span: Span, message: impl Into<String>) -> Self {
        Self { span, message: message.into(), is_primary: true }
    }

    pub fn secondary(span: Span, message: impl Into<String>) -> Self {
        Self { span, message: message.into(), is_primary: false }
    }
}

/// A machine-applicable edit that would resolve the diagnostic.
///
/// The CLI prints these; the language server offers them as code
/// actions. Both consume the same value, so a fix cannot be available in
/// one and missing in the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fix {
    pub description: String,
    pub span: Span,
    /// Text to substitute for `span`. Empty means deletion.
    pub replacement: String,
}

impl Fix {
    pub fn new(description: impl Into<String>, span: Span, replacement: impl Into<String>) -> Self {
        Self { description: description.into(), span, replacement: replacement.into() }
    }
}

/// One problem, with everything needed to explain and possibly fix it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: DiagnosticCode,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub fixes: Vec<Fix>,
}

impl Diagnostic {
    pub fn new(severity: Severity, code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity,
            code,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            fixes: Vec::new(),
        }
    }

    pub fn error(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::new(Severity::Error, code, message)
    }

    pub fn warning(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, code, message)
    }

    #[must_use]
    pub fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.notes.push(note.into());
        self
    }

    #[must_use]
    pub fn with_fix(mut self, fix: Fix) -> Self {
        self.fixes.push(fix);
        self
    }

    /// The first line of rendered output: `error[E3001]: missing STX`.
    #[must_use]
    pub fn headline(&self) -> String {
        format!("{}[{}]: {}", self.severity, self.code, self.message)
    }

    /// The location the diagnostic is about, if any.
    #[must_use]
    pub fn primary_span(&self) -> Option<Span> {
        self.labels.iter().find(|label| label.is_primary).map(|label| label.span)
    }
}

/// An ordered collection of diagnostics from one operation.
///
/// Insertion order is preserved and is the reporting order: it is the
/// order the compiler found the problems, which is the order that makes
/// sense to read. Sorting or hash-map iteration here would make output
/// nondeterministic, which CLAUDE.md forbids reaching output.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticBundle {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBundle {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn extend(&mut self, other: impl IntoIterator<Item = Diagnostic>) {
        self.diagnostics.extend(other);
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Diagnostic> {
        self.diagnostics.iter()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Whether anything here should fail the operation.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    /// Promote every warning to an error, for `--deny-warnings`
    /// (SPEC.md §5.16.3).
    ///
    /// A property of the bundle rather than something each call site
    /// remembers to check — one place to get right instead of many.
    pub fn deny_warnings(&mut self) {
        for diagnostic in &mut self.diagnostics {
            if diagnostic.severity == Severity::Warning {
                diagnostic.severity = Severity::Error;
            }
        }
    }
}

impl IntoIterator for DiagnosticBundle {
    type Item = Diagnostic;
    type IntoIter = std::vec::IntoIter<Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.diagnostics.into_iter()
    }
}

impl<'a> IntoIterator for &'a DiagnosticBundle {
    type Item = &'a Diagnostic;
    type IntoIter = std::slice::Iter<'a, Diagnostic>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl FromIterator<Diagnostic> for DiagnosticBundle {
    fn from_iter<T: IntoIterator<Item = Diagnostic>>(iter: T) -> Self {
        Self { diagnostics: iter.into_iter().collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FileId, Span, TextRange};

    fn span() -> Span {
        Span::new(FileId(0), TextRange::new(3, 9))
    }

    #[test]
    fn codes_render_in_the_spec_form() {
        // SPEC.md §5.18 shows `E0204`, `E1302`, `E2207`: always `E`,
        // always four digits, zero-padded.
        assert_eq!(DiagnosticCode::new(204).to_string(), "E0204");
        assert_eq!(DiagnosticCode::new(1302).to_string(), "E1302");
        assert_eq!(DiagnosticCode::new(2207).to_string(), "E2207");
        assert_eq!(DiagnosticCode::new(1).to_string(), "E0001");
    }

    #[test]
    fn codes_expose_their_category() {
        use crate::codes::Category;
        assert_eq!(DiagnosticCode::new(204).category(), Category::Syntax);
        assert_eq!(DiagnosticCode::new(1302).category(), Category::Semantics);
        assert_eq!(DiagnosticCode::new(2207).category(), Category::Fitting);
        assert_eq!(DiagnosticCode::new(3001).category(), Category::Jedec);
    }

    #[test]
    fn a_diagnostic_headline_names_severity_code_and_message() {
        let diagnostic = Diagnostic::error(DiagnosticCode::new(3001), "missing STX");
        assert_eq!(diagnostic.headline(), "error[E3001]: missing STX");
    }

    #[test]
    fn warnings_say_warning() {
        let diagnostic = Diagnostic::warning(DiagnosticCode::new(3100), "unknown field `Z`");
        assert_eq!(diagnostic.headline(), "warning[E3100]: unknown field `Z`");
        assert_eq!(diagnostic.severity, Severity::Warning);
    }

    #[test]
    fn labels_and_notes_attach_in_order() {
        let diagnostic = Diagnostic::error(DiagnosticCode::new(3020), "bad fuse count")
            .with_label(Label::primary(span(), "expected a decimal number"))
            .with_label(Label::secondary(span(), "field started here"))
            .with_note("QF must precede any L field")
            .with_note("see JEDEC 3A, Value Fields");

        assert_eq!(diagnostic.labels.len(), 2);
        assert!(diagnostic.labels[0].is_primary);
        assert!(!diagnostic.labels[1].is_primary);
        assert_eq!(diagnostic.notes, ["QF must precede any L field", "see JEDEC 3A, Value Fields"]);
    }

    #[test]
    fn the_primary_span_is_the_first_primary_label() {
        let diagnostic = Diagnostic::error(DiagnosticCode::new(3020), "bad fuse count")
            .with_label(Label::secondary(Span::new(FileId(0), TextRange::new(0, 2)), "here"))
            .with_label(Label::primary(span(), "and here"));
        assert_eq!(diagnostic.primary_span(), Some(span()));
    }

    #[test]
    fn a_diagnostic_without_labels_has_no_primary_span() {
        let diagnostic = Diagnostic::error(DiagnosticCode::new(3001), "missing STX");
        assert_eq!(diagnostic.primary_span(), None);
    }

    #[test]
    fn fixes_carry_a_replacement_and_a_description() {
        let fix = Fix::new("insert the STX character", span(), "\u{2}");
        let diagnostic = Diagnostic::error(DiagnosticCode::new(3001), "missing STX").with_fix(fix);
        assert_eq!(diagnostic.fixes.len(), 1);
        assert_eq!(diagnostic.fixes[0].replacement, "\u{2}");
    }

    #[test]
    fn a_bundle_is_errored_only_when_it_holds_an_error() {
        let mut bundle = DiagnosticBundle::default();
        assert!(!bundle.has_errors());
        assert!(bundle.is_empty());

        bundle.push(Diagnostic::warning(DiagnosticCode::new(3100), "unknown field"));
        assert!(!bundle.has_errors(), "a warning alone must not fail a build");
        assert!(!bundle.is_empty());

        bundle.push(Diagnostic::error(DiagnosticCode::new(3001), "missing STX"));
        assert!(bundle.has_errors());
        assert_eq!(bundle.len(), 2);
    }

    #[test]
    fn a_bundle_preserves_insertion_order() {
        // Diagnostics are reported in the order the compiler found them.
        // Reordering (or worse, hash-map order) would make output
        // nondeterministic, which CLAUDE.md forbids reaching output.
        let mut bundle = DiagnosticBundle::default();
        for n in [3001u16, 3002, 3003] {
            bundle.push(Diagnostic::error(DiagnosticCode::new(n), "boom"));
        }
        let codes: Vec<_> = bundle.iter().map(|d| d.code.to_string()).collect();
        assert_eq!(codes, ["E3001", "E3002", "E3003"]);
    }

    #[test]
    fn deny_warnings_promotes_warnings_to_errors() {
        // `--deny-warnings` (SPEC.md §5.16.3) must be a property of the
        // bundle, not something each call site remembers to check.
        let mut bundle = DiagnosticBundle::default();
        bundle.push(Diagnostic::warning(DiagnosticCode::new(3100), "unknown field"));
        assert!(!bundle.has_errors());

        bundle.deny_warnings();
        assert!(bundle.has_errors());
        assert_eq!(bundle.iter().next().unwrap().severity, Severity::Error);
    }
}
