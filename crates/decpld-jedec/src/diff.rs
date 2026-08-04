//! Comparing two JEDEC files.
//!
//! Compares *fuse vectors*, not file text (SPEC.md §7.5). Two files can
//! describe an identical device while sharing barely a byte — different
//! `L` groupings, line endings, header, or field order. Diffing text
//! would report all of that as change and bury the one fuse that
//! actually moved.

use crate::model::JedecFile;

/// One fuse whose state differs between two files.
///
/// SPEC.md §7.5 types the index as `FuseId`; that is a device-layer
/// concept, and this crate is architecture-free by construction, so a
/// bare fuse number is used here. Classifying a delta as "polarity" or
/// "mode" requires knowing what the fuse means, and belongs to the
/// target that does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FuseDelta {
    pub index: u32,
    pub before: bool,
    pub after: bool,
}

/// What differs between two files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JedecDiff {
    /// Differing fuse counts. When set, `fuses` is empty: comparing
    /// fuse *N* across devices of different sizes compares nothing
    /// meaningful, and pretending otherwise would produce a wall of
    /// spurious deltas that hides the real finding.
    pub fuse_count: Option<(u32, u32)>,
    pub fuses: Vec<FuseDelta>,
    pub default_fuse: Option<(Option<bool>, Option<bool>)>,
    pub security: Option<(Option<bool>, Option<bool>)>,
    pub design_specification: Option<(String, String)>,
    pub notes: Option<(Vec<String>, Vec<String>)>,
    /// Fields deCPLD does not model, as `identifier body` pairs.
    ///
    /// Compared because [`JedecFile::describes_same_device_as`] compares
    /// them, and two notions of "the same file" that disagree would let
    /// `jed diff` bless a rewrite that silently deleted a device's test
    /// vectors.
    pub unknown_fields: Option<(Vec<String>, Vec<String>)>,
}

impl JedecDiff {
    /// Whether the two files describe the same device.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fuse_count.is_none()
            && self.fuses.is_empty()
            && self.default_fuse.is_none()
            && self.security.is_none()
            && self.design_specification.is_none()
            && self.notes.is_none()
            && self.unknown_fields.is_none()
    }
}

/// Compare two files.
#[must_use]
pub fn diff(before: &JedecFile, after: &JedecFile) -> JedecDiff {
    let mut out = JedecDiff::default();

    if before.design_specification != after.design_specification {
        out.design_specification =
            Some((before.design_specification.clone(), after.design_specification.clone()));
    }
    if before.default_fuse != after.default_fuse {
        out.default_fuse = Some((before.default_fuse, after.default_fuse));
    }
    if before.security != after.security {
        out.security = Some((before.security, after.security));
    }
    if before.notes != after.notes {
        out.notes = Some((before.notes.clone(), after.notes.clone()));
    }
    if before.unknown_fields != after.unknown_fields {
        out.unknown_fields =
            Some((describe(&before.unknown_fields), describe(&after.unknown_fields)));
    }

    if before.fuses.len() != after.fuses.len() {
        out.fuse_count = Some((before.fuses.len(), after.fuses.len()));
        return out;
    }

    for (index, (a, b)) in before.fuses.iter().zip(after.fuses.iter()).enumerate() {
        if a != b {
            out.fuses.push(FuseDelta { index: index as u32, before: a, after: b });
        }
    }

    out
}

/// Render unmodelled fields for reporting.
///
/// Shows exactly what is compared — bodies are already normalised at
/// parse time, so no trimming happens here. A renderer that tidied its
/// output would be able to print `["QP20"] -> ["QP20"]` for a reported
/// difference, which is worse than either answer on its own.
fn describe(fields: &[crate::JedecField]) -> Vec<String> {
    fields.iter().map(|f| format!("{}{}", f.identifier, f.body)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FuseVector, parse};
    use decpld_diagnostics::FileId;

    fn file(text: &str) -> JedecFile {
        parse(text, FileId(0)).expect("fixture parses").file
    }

    #[test]
    fn identical_files_have_no_diff() {
        let a = file("\x02h*QF16*F0*L0 1010000000000000*\x030000");
        assert!(diff(&a, &a).is_empty());
    }

    #[test]
    fn formatting_differences_are_not_differences() {
        // The point of diffing fuse vectors rather than text: these two
        // files share almost no bytes and describe one device.
        let a = file("\x02h*QF16*F0*L0 1010000000000000*\x030000");
        let b = file("\x02h*\r\nQF16*\r\nF0*\r\nL0 10100000*\r\nL8 00000000*\r\n\x030000");
        assert!(diff(&a, &b).is_empty(), "{:#?}", diff(&a, &b));
    }

    #[test]
    fn a_single_changed_fuse_is_reported_once() {
        let a = file("\x02h*QF16*F0*L0 1010000000000000*\x030000");
        let b = file("\x02h*QF16*F0*L0 1011000000000000*\x030000");
        let d = diff(&a, &b);
        assert_eq!(d.fuses, [FuseDelta { index: 3, before: false, after: true }]);
    }

    #[test]
    fn a_default_state_flip_is_reported_as_such() {
        // F0 vs F1 with the same explicit fuses: every unlisted fuse
        // moves, but the *reason* is one field, and reporting it is more
        // useful than listing every consequence.
        let a = file("\x02h*QF16*F0*L0 1010000000000000*\x030000");
        let b = file("\x02h*QF16*F1*L0 1010000000000000*\x030000");
        let d = diff(&a, &b);
        assert_eq!(d.default_fuse, Some((Some(false), Some(true))));
    }

    #[test]
    fn differing_fuse_counts_suppress_the_fuse_comparison() {
        // Fuse 5 of a 16-fuse device and fuse 5 of a 32-fuse device are
        // not the same fuse. Listing deltas between them would be a wall
        // of noise hiding the one thing worth saying.
        let a = file("\x02h*QF16*F0*\x030000");
        let b = file("\x02h*QF32*F0*\x030000");
        let d = diff(&a, &b);
        assert_eq!(d.fuse_count, Some((16, 32)));
        assert!(d.fuses.is_empty());
        assert!(!d.is_empty());
    }

    #[test]
    fn a_dropped_unmodelled_field_is_a_difference() {
        // `describes_same_device_as` counts unmodelled fields, and `diff`
        // must agree with it: two notions of "the same file" that
        // disagree would make `jed diff` bless a rewrite that silently
        // deleted a device's test vectors — the exact loss
        // preserve-unknown mode exists to prevent.
        let a = file("\x02h*QF8*F0*L0 11110000*V0001 XXXX*\x030000");
        let b = file("\x02h*QF8*F0*L0 11110000*\x030000");
        let d = diff(&a, &b);
        assert!(!d.is_empty(), "dropping a test vector is a difference");
        assert!(d.unknown_fields.is_some(), "{d:#?}");
        assert!(d.fuses.is_empty(), "the fuses did not change");
    }

    #[test]
    fn diff_agrees_with_describes_same_device_as() {
        // The two must never disagree. Stated directly so a future
        // change to one is caught if it forgets the other.
        let cases = [
            ("\x02h*QF8*F0*L0 11110000*\x030000", "\x02h*QF8*F0*L0 11110000*\x030000", true),
            ("\x02h*QF8*F0*L0 11110000*\x030000", "\x02h*QF8*F0*L0 11110001*\x030000", false),
            ("\x02h*QF8*F0*V1 X*\x030000", "\x02h*QF8*F0*\x030000", false),
            ("\x02one*QF8*F0*\x030000", "\x02two*QF8*F0*\x030000", false),
            ("\x02h*QF8*F0*G1*\x030000", "\x02h*QF8*F0*\x030000", false),
            // `default_fuse` became an Option, so the agreement has to
            // hold across all four pairings, not just the two it used
            // to have. Silence and an explicit F0 describe different
            // files even when every fuse ends up identical.
            ("\x02h*QF8*L0 00000000*\x030000", "\x02h*QF8*F0*L0 00000000*\x030000", false),
            ("\x02h*QF8*L0 00000000*\x030000", "\x02h*QF8*F1*L0 00000000*\x030000", false),
            ("\x02h*QF8*L0 10110001*\x030000", "\x02h*QF8*L0 10110001*\x030000", true),
        ];
        for (left, right, same) in cases {
            let (a, b) = (file(left), file(right));
            assert_eq!(
                diff(&a, &b).is_empty(),
                a.describes_same_device_as(&b),
                "disagreement on {left:?} vs {right:?}"
            );
            assert_eq!(diff(&a, &b).is_empty(), same, "wrong verdict on {left:?}");
        }
    }

    #[test]
    fn metadata_differences_are_reported_separately() {
        let a = file("\x02one*QF8*F0*G0*N first*\x030000");
        let b = file("\x02two*QF8*F0*G1*N second*\x030000");
        let d = diff(&a, &b);
        assert_eq!(d.design_specification, Some(("one".into(), "two".into())));
        assert_eq!(d.security, Some((Some(false), Some(true))));
        assert_eq!(d.notes, Some((vec!["first".to_owned()], vec!["second".to_owned()])));
        assert!(d.fuses.is_empty(), "metadata changes are not fuse changes");
    }

    #[test]
    fn every_differing_fuse_is_found() {
        // Exhaustive over a small device: for each fuse, flipping only
        // that one must produce exactly that one delta. Catches
        // off-by-one and word-boundary errors that a single spot-check
        // would miss.
        for flipped in 0..24u32 {
            let base = FuseVector::new(24, false);
            let mut changed = FuseVector::new(24, false);
            changed.set(flipped, true).unwrap();

            let a = JedecFile {
                design_specification: String::new(),
                fuses: base,
                default_fuse: Some(false),
                notes: Vec::new(),
                security: None,
                fuse_checksum: None,
                transmission_checksum: None,
                unknown_fields: Vec::new(),
            };
            let b = JedecFile { fuses: changed, ..a.clone() };

            let d = diff(&a, &b);
            assert_eq!(
                d.fuses,
                [FuseDelta { index: flipped, before: false, after: true }],
                "flipping fuse {flipped}"
            );
        }
    }
}
