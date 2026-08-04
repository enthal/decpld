//! The parsed contents of a JEDEC file. SPEC.md §5.6.

use crate::FuseVector;
use decpld_diagnostics::Span;

/// A field the parser recognised structurally but does not model.
///
/// Retained verbatim so a file can be rewritten without losing anything
/// deCPLD did not happen to understand — test vectors, pin lists,
/// signature-analysis fields, and vendor extensions all live here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JedecField {
    /// The leading identifier, e.g. `QP`, `V`, `X`.
    pub identifier: String,
    /// Everything between the identifier and the terminating `*`.
    pub body: String,
    pub span: Span,
}

/// One JEDEC file.
///
/// Deliberately architecture-free: this is fuse *numbers* and states,
/// notes, and checksums. Nothing here knows what any fuse means. That is
/// the layering rule, and it is what lets the crate read a file for a
/// device it has never heard of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JedecFile {
    /// The free-text header between STX and the first `*`.
    ///
    /// `None` only when the file had no header at all; an empty header
    /// is `Some("")`, because "present and blank" and "absent" are
    /// different facts about a file being round-tripped.
    pub design_specification: Option<String>,

    /// Fuse states for the whole device.
    ///
    /// The fuse count lives here rather than in a separate field, so a
    /// `fuse_count` that disagrees with the vector length is not a state
    /// this type can represent. (SPEC.md §5.6 lists them separately;
    /// collapsing them is recorded there as a deliberate deviation.)
    pub fuses: FuseVector,

    /// The `F` field: the state every fuse starts in.
    pub default_fuse: bool,

    /// `N` fields, in file order.
    pub notes: Vec<String>,

    /// The `G` security fuse. `None` means the file did not mention it,
    /// which is not the same as `Some(false)` — one is silence, the
    /// other is an explicit instruction to leave the part readable.
    pub security: Option<bool>,

    /// The `C` field as written, whether or not it matched.
    pub fuse_checksum: Option<u16>,

    /// The four hex digits after ETX, if present.
    pub transmission_checksum: Option<u16>,

    /// Fields retained but not modelled. See [`JedecField`].
    pub unknown_fields: Vec<JedecField>,
}

impl JedecFile {
    /// The fuse checksum this file's data actually produces.
    ///
    /// Compare with [`Self::fuse_checksum`] to decide whether the `C`
    /// field is telling the truth.
    #[must_use]
    pub fn computed_fuse_checksum(&self) -> u16 {
        self.fuses.checksum()
    }
}
