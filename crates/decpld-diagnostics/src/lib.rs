//! Source locations and structured diagnostics.
//!
//! Shared by every deCPLD crate. The CLI and the language server render
//! the same [`Diagnostic`] values, which is what keeps `decpld check`
//! and an editor's red squiggle from ever disagreeing (CLAUDE.md → the
//! one architectural rule).
//!
//! Types follow SPEC.md §3.2 (spans) and §5.18 (diagnostics).

mod diagnostic;
mod line_index;
mod span;

pub mod codes;

pub use diagnostic::{Diagnostic, DiagnosticBundle, DiagnosticCode, Fix, Label, Severity};
pub use line_index::{LineCol, LineIndex};
pub use span::{FileId, Span, TextRange};
