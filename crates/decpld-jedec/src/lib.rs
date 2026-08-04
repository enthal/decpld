//! JEDEC fuse-map files.
//!
//! JEDEC transfers numbered fuse/cell states; it does **not** define
//! device architecture (SPEC.md §5.6). Nothing in this crate knows what
//! a macrocell is, which fuse selects a polarity, or that ATF22V10
//! exists. That is the layering rule, and it is also what makes the
//! crate useful: it can read a file for a part it has never heard of.
//!
//! Primary reference: JEDEC Standard No. 3A, recorded in
//! `targets/evidence/references.toml` as `jedec-3a`.

mod checksum;
mod diff;
mod fuses;
mod model;
mod parse;
mod write;

pub mod codes;

pub use checksum::{DUMMY_TRANSMISSION_CHECKSUM, transmission_checksum};
pub use diff::{FuseDelta, JedecDiff, diff};
pub use fuses::{FuseError, FuseVector};
pub use model::{JedecField, JedecFile};
pub use parse::{Parsed, ParserMode, parse, parse_with_mode};
pub use write::{WriteError, WriterStyle, write};

/// JEDEC 3A's `<field character>` class, lines 158-160:
///
/// ```text
/// <field character> ::= <ASCII 20 hex ... 29 hex>
///                   |   <ASCII 2B hex ... 7E hex>
///                   |   <carriage return> | <line feed>
/// ```
///
/// The gap at `0x2A` is the field terminator `*`, so this one predicate
/// covers the asterisk case too and there is no second place for the two
/// to drift. Note it stops at `0x7E`: JEDEC predates Unicode, and
/// non-ASCII genuinely cannot be encoded.
///
/// Shared by the parser and the writer deliberately. The writer refuses
/// what it cannot encode; the parser reports it at an offset, where a
/// caret can point at it. Two copies of this class would eventually
/// disagree about which files are writable.
///
/// Evidence: `jedec-3a` (sha256 `9207f92b…` in
/// `targets/evidence/references.toml`).
#[must_use]
pub fn is_field_character(ch: char) -> bool {
    matches!(ch, '\u{20}'..='\u{29}' | '\u{2B}'..='\u{7E}' | '\r' | '\n')
}
