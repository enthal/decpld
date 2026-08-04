//! Diagnostic code ranges, and the codes owned by this crate.
//!
//! Codes are **stable forever** (CLAUDE.md → code style). Once `E0204`
//! means "value does not fit", it always means that. Add new codes;
//! never renumber, because a code that changes meaning silently
//! invalidates every issue report, script, and `--deny` list quoting it.
//!
//! The ranges below are derived from the three worked examples in
//! SPEC.md §5.18 — `E0204` (a width error), `E1302` (a clock
//! requirement), `E2207` (an ATF16V8 mode failure) — and extended to
//! cover the remaining layers.

use crate::DiagnosticCode;

/// The layer a diagnostic comes from, decided by its thousands digit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    /// `0xxx` — lexing, parsing, types, widths, signedness, literals.
    Syntax,
    /// `1xxx` — producers, storage, clocks, pads, visibility, packages.
    Semantics,
    /// `2xxx` — fitting, device resources, mode selection.
    Fitting,
    /// `3xxx` — JEDEC parsing, writing, checksums, fuse encoding.
    Jedec,
    /// `4xxx` — CLI, manifest, and I/O.
    Driver,
    /// `9xxx` — internal compiler errors. Reaching one is always a bug
    /// in deCPLD, never in the user's design.
    Internal,
    /// Outside every declared range.
    ///
    /// Explicit rather than folded into a neighbouring category: a
    /// mis-numbered code should be visible, not quietly absorbed.
    Unclassified,
}

impl Category {
    #[must_use]
    pub const fn of(code: u16) -> Self {
        match code {
            0..=999 => Self::Syntax,
            1000..=1999 => Self::Semantics,
            2000..=2999 => Self::Fitting,
            3000..=3999 => Self::Jedec,
            4000..=4999 => Self::Driver,
            9000..=9999 => Self::Internal,
            _ => Self::Unclassified,
        }
    }
}

/// An invariant deCPLD believed about itself did not hold. Always a
/// compiler bug.
pub const INTERNAL: DiagnosticCode = DiagnosticCode::new(9000);

/// Every code owned by this crate, so the uniqueness and range tests can
/// see all of them. Each crate keeps its own list.
pub const ALL: &[DiagnosticCode] = &[INTERNAL];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DiagnosticCode;

    #[test]
    fn category_boundaries_follow_the_thousands_digit() {
        assert_eq!(Category::of(0), Category::Syntax);
        assert_eq!(Category::of(999), Category::Syntax);
        assert_eq!(Category::of(1000), Category::Semantics);
        assert_eq!(Category::of(1999), Category::Semantics);
        assert_eq!(Category::of(2000), Category::Fitting);
        assert_eq!(Category::of(3000), Category::Jedec);
        assert_eq!(Category::of(4000), Category::Driver);
        assert_eq!(Category::of(9000), Category::Internal);
    }

    #[test]
    fn codes_outside_a_defined_range_are_unclassified() {
        // Better an explicit `Unclassified` than silently folding a
        // stray code into a neighbouring category and hiding the
        // mistake.
        assert_eq!(Category::of(5000), Category::Unclassified);
        assert_eq!(Category::of(8999), Category::Unclassified);
    }

    #[test]
    fn spec_example_codes_land_in_their_documented_categories() {
        // The three examples in SPEC.md §5.18 are the check that these
        // ranges were derived from the spec rather than invented.
        assert_eq!(Category::of(204), Category::Syntax); // value does not fit
        assert_eq!(Category::of(1302), Category::Semantics); // needs global clock
        assert_eq!(Category::of(2207), Category::Fitting); // no ATF16V8 mode
    }

    #[test]
    fn every_code_this_crate_owns_is_unique() {
        // Codes are stable forever (CLAUDE.md → code style), so a
        // duplicate would silently give two different failures the same
        // identity. Catch it at test time rather than in a bug report.
        let mut seen = std::collections::BTreeSet::new();
        for code in ALL {
            assert!(seen.insert(code.as_u16()), "duplicate diagnostic code {code}");
        }
    }

    #[test]
    fn every_code_this_crate_owns_is_in_a_declared_range() {
        for code in ALL {
            assert_ne!(
                code.category(),
                Category::Unclassified,
                "{code} is outside every declared range"
            );
        }
    }

    #[test]
    fn internal_error_code_is_stable() {
        assert_eq!(INTERNAL.to_string(), "E9000");
        assert_eq!(DiagnosticCode::new(9000), INTERNAL);
    }
}
