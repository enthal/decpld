//! The parsed contents of a JEDEC file. SPEC.md §5.6.

use crate::FuseVector;
use decpld_diagnostics::Span;

/// A field the parser recognised structurally but does not model.
///
/// Retained verbatim so a file can be rewritten without losing anything
/// deCPLD did not happen to understand — test vectors, pin lists,
/// signature-analysis fields, and vendor extensions all live here.
#[derive(Clone, Debug, Eq)]
pub struct JedecField {
    /// The leading identifier, e.g. `QP`, `V`, `X`.
    pub identifier: String,
    /// Everything between the identifier and the terminating `*`.
    pub body: String,
    /// Where it was found. Diagnostics need this; identity does not.
    pub span: Span,
}

/// Two fields are the same field when they say the same thing.
///
/// `span` is deliberately excluded. A field that survives a rewrite
/// lands at a different offset, and if position counted as identity then
/// no file could ever round-trip — which would make the round-trip test,
/// the most valuable test in this crate, impossible to state.
impl PartialEq for JedecField {
    fn eq(&self, other: &Self) -> bool {
        self.identifier == other.identifier && self.body == other.body
    }
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
    /// Not optional, because JEDEC cannot express its absence: the
    /// header IS the first field, so the shortest possible file,
    /// `<STX>*...`, still has one — an empty one. Modelling it as
    /// `Option` let the type say something the format could not, and a
    /// round-trip property test caught exactly that: a `None` header
    /// came back as `Some("")` because there was nowhere to put the
    /// difference.
    pub design_specification: String,

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

    /// Whether two files describe the same device configuration,
    /// ignoring the checksums.
    ///
    /// This is the right notion of sameness for a rewrite. Both
    /// checksums are *derived*: the fuse checksum from the fuse data,
    /// the transmission checksum from the emitted bytes. A writer that
    /// preserved them verbatim would be preserving a defect — a file
    /// carrying `C0000` ("not computed") should come out of
    /// canonicalisation with a real checksum, and a file whose bytes
    /// changed must carry a new transmission checksum or it fails its
    /// own verification.
    ///
    /// So `write` deliberately does not round-trip those two fields, and
    /// a round-trip test that demanded it would be asserting the wrong
    /// invariant. Everything that is genuinely *content* — header,
    /// fuses, default state, notes, security bit, and unmodelled
    /// fields — must survive exactly.
    #[must_use]
    pub fn describes_same_device_as(&self, other: &Self) -> bool {
        self.design_specification == other.design_specification
            && self.fuses == other.fuses
            && self.default_fuse == other.default_fuse
            && self.notes == other.notes
            && self.security == other.security
            && self.unknown_fields == other.unknown_fields
    }
}
