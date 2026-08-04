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
mod fuses;
mod model;
mod parse;

pub mod codes;

pub use checksum::{DUMMY_TRANSMISSION_CHECKSUM, transmission_checksum};
pub use fuses::{FuseError, FuseVector};
pub use model::{JedecField, JedecFile};
pub use parse::{Parsed, ParserMode, parse, parse_with_mode};
