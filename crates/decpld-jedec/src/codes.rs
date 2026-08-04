//! Diagnostic codes owned by the JEDEC layer (`3xxx`, SPEC.md §5.18.1).
//!
//! Codes are permanent. Add new ones; never renumber.

use decpld_diagnostics::DiagnosticCode;

// ---- Framing ----

/// No STX (`0x02`) anywhere in the input.
pub const MISSING_STX: DiagnosticCode = DiagnosticCode::new(3001);
/// No ETX (`0x03`) after the STX.
pub const MISSING_ETX: DiagnosticCode = DiagnosticCode::new(3002);
/// A field ran to ETX without its terminating `*`.
pub const UNTERMINATED_FIELD: DiagnosticCode = DiagnosticCode::new(3003);

// ---- Fuse count and fuse list ----

/// A fuse number lies outside the count declared by `QF`.
pub const FUSE_OUT_OF_RANGE: DiagnosticCode = DiagnosticCode::new(3010);
/// An `L` field appeared before any `QF`, so the device size is unknown.
pub const FUSE_LIST_BEFORE_FUSE_COUNT: DiagnosticCode = DiagnosticCode::new(3011);
/// A fuse state character was neither `0` nor `1`.
pub const INVALID_FUSE_STATE: DiagnosticCode = DiagnosticCode::new(3012);
/// A field expected a decimal number and did not get one.
pub const INVALID_NUMBER: DiagnosticCode = DiagnosticCode::new(3013);
/// More than one `QF` field.
pub const DUPLICATE_FUSE_COUNT: DiagnosticCode = DiagnosticCode::new(3014);
/// The file declares fuses but never says how many.
pub const MISSING_FUSE_COUNT: DiagnosticCode = DiagnosticCode::new(3015);
/// An `L` field ran its fuse number straight into its fuse states with
/// no separator, so where the number ends cannot be determined.
pub const AMBIGUOUS_FUSE_LIST: DiagnosticCode = DiagnosticCode::new(3016);
/// `QF` declares more fuses than deCPLD will allocate for.
pub const FUSE_COUNT_TOO_LARGE: DiagnosticCode = DiagnosticCode::new(3017);

// ---- Checksums ----

/// A `C` field was not four hexadecimal digits.
pub const INVALID_CHECKSUM_FIELD: DiagnosticCode = DiagnosticCode::new(3020);
/// The `C` field disagrees with the fuse data.
pub const FUSE_CHECKSUM_MISMATCH: DiagnosticCode = DiagnosticCode::new(3021);
/// The trailing transmission checksum disagrees with the bytes sent.
pub const TRANSMISSION_CHECKSUM_MISMATCH: DiagnosticCode = DiagnosticCode::new(3022);

// ---- Other fields ----

/// An `F` field was neither `F0` nor `F1`.
pub const INVALID_DEFAULT_STATE: DiagnosticCode = DiagnosticCode::new(3030);
/// A `G` field was neither `G0` nor `G1`.
pub const INVALID_SECURITY_FIELD: DiagnosticCode = DiagnosticCode::new(3031);
/// More than one `F` field, all naming the same state. A grammar
/// violation with only one possible meaning, so it is a warning.
pub const DUPLICATE_DEFAULT_STATE: DiagnosticCode = DiagnosticCode::new(3032);
/// An `F` field appeared after an `L` field, so which fuses it governs
/// depends on reading order.
pub const DEFAULT_STATE_AFTER_FUSE_LIST: DiagnosticCode = DiagnosticCode::new(3033);
/// Two `F` fields name *different* default states, so the file has no
/// single meaning. Distinct from [`DUPLICATE_DEFAULT_STATE`]: repetition
/// is untidy, disagreement is unresolvable.
pub const CONTRADICTORY_DEFAULT_STATE: DiagnosticCode = DiagnosticCode::new(3034);
/// More than one `G` field, all naming the same state.
pub const DUPLICATE_SECURITY_FIELD: DiagnosticCode = DiagnosticCode::new(3035);
/// Two `G` fields disagree about the security fuse. Never resolved by
/// last-writer-wins: the security fuse is irreversible, and inferring
/// "lock the part" from a self-contradictory file is the one guess this
/// crate must never make.
pub const CONTRADICTORY_SECURITY_FIELD: DiagnosticCode = DiagnosticCode::new(3036);

/// A field identifier that JEDEC 3A does not define.
pub const UNKNOWN_FIELD: DiagnosticCode = DiagnosticCode::new(3040);
/// A field deCPLD does not model was discarded rather than retained.
pub const FIELD_DISCARDED: DiagnosticCode = DiagnosticCode::new(3041);
/// The file ends at ETX with no transmission checksum.
pub const MISSING_TRANSMISSION_CHECKSUM: DiagnosticCode = DiagnosticCode::new(3042);
/// A character outside JEDEC 3A's `<field character>` class appeared
/// inside a field. The writer cannot encode it, so the parser reports it
/// where it can still be pointed at.
pub const INVALID_FIELD_CHARACTER: DiagnosticCode = DiagnosticCode::new(3043);

/// Every code owned by this crate, for the uniqueness and range tests.
pub const ALL: &[DiagnosticCode] = &[
    MISSING_STX,
    MISSING_ETX,
    UNTERMINATED_FIELD,
    FUSE_OUT_OF_RANGE,
    FUSE_LIST_BEFORE_FUSE_COUNT,
    INVALID_FUSE_STATE,
    INVALID_NUMBER,
    DUPLICATE_FUSE_COUNT,
    MISSING_FUSE_COUNT,
    AMBIGUOUS_FUSE_LIST,
    FUSE_COUNT_TOO_LARGE,
    INVALID_CHECKSUM_FIELD,
    FUSE_CHECKSUM_MISMATCH,
    TRANSMISSION_CHECKSUM_MISMATCH,
    INVALID_DEFAULT_STATE,
    INVALID_SECURITY_FIELD,
    DUPLICATE_DEFAULT_STATE,
    DEFAULT_STATE_AFTER_FUSE_LIST,
    CONTRADICTORY_DEFAULT_STATE,
    DUPLICATE_SECURITY_FIELD,
    CONTRADICTORY_SECURITY_FIELD,
    UNKNOWN_FIELD,
    FIELD_DISCARDED,
    MISSING_TRANSMISSION_CHECKSUM,
    INVALID_FIELD_CHARACTER,
];

#[cfg(test)]
mod tests {
    use super::*;
    use decpld_diagnostics::codes::Category;

    #[test]
    fn every_code_this_crate_owns_is_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for code in ALL {
            assert!(seen.insert(code.as_u16()), "duplicate diagnostic code {code}");
        }
    }

    #[test]
    fn every_code_this_crate_owns_is_in_the_jedec_range() {
        // A JEDEC diagnostic numbered outside 3xxx would report as
        // belonging to another layer, which is exactly the confusion the
        // ranges exist to prevent.
        for code in ALL {
            assert_eq!(code.category(), Category::Jedec, "{code} is not in the JEDEC range");
        }
    }
}
