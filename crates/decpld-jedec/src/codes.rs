//! Diagnostic codes owned by the JEDEC layer (`3xxx`, SPEC.md §5.18.1).

use decpld_diagnostics::DiagnosticCode;

/// A fuse number lies outside the count declared by `QF`.
pub const FUSE_OUT_OF_RANGE: DiagnosticCode = DiagnosticCode::new(3010);

/// Every code owned by this crate, for the uniqueness and range tests.
pub const ALL: &[DiagnosticCode] = &[FUSE_OUT_OF_RANGE];

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
