//! How well a device fact is established. SPEC.md §13.1.

use decpld_device::{Evidence, EvidenceLevel};

#[test]
fn the_ordering_counts_witnesses_it_does_not_rank_authority() {
    // SPEC.md §13.1 is explicit that this is a count, not a league
    // table. `DatasheetSpecified` sits below `DifferentiallyVerified`
    // because one document is one witness — not because a vendor is
    // less trustworthy than an oracle run. For a fact no fuse
    // experiment can observe, such as which pin is bonded to ground,
    // the datasheet is the better witness and the only one available.
    let ascending = [
        EvidenceLevel::Hypothesis,
        EvidenceLevel::DatasheetSpecified,
        EvidenceLevel::DifferentiallyVerified,
        EvidenceLevel::OpenSourceCrossChecked,
        EvidenceLevel::HardwareVerified,
    ];
    for pair in ascending.windows(2) {
        assert!(pair[0] < pair[1], "{:?} must sort below {:?}", pair[0], pair[1]);
    }
    assert_eq!(EvidenceLevel::ALL, ascending);
}

#[test]
fn a_hypothesis_never_meets_the_production_threshold() {
    // CLAUDE.md: "Unverified hypotheses belong in oracle-analysis code
    // or disabled experimental targets, never in a production target
    // definition." The threshold is what makes that checkable rather
    // than a habit.
    assert!(!EvidenceLevel::Hypothesis.is_production_ready());
    for level in EvidenceLevel::ALL.into_iter().skip(1) {
        assert!(level.is_production_ready(), "{level:?}");
    }
    assert_eq!(EvidenceLevel::PRODUCTION_THRESHOLD, EvidenceLevel::DatasheetSpecified);
}

#[test]
fn evidence_carries_the_sources_that_established_it() {
    // A level with nothing behind it is a claim about a claim. Naming
    // the experiments is what lets a reader re-derive the fact, and
    // what lets every test that trusted an oracle run be found if the
    // run is later shown wrong.
    let evidence =
        Evidence::new(EvidenceLevel::DifferentiallyVerified, &["arch-comb-high", "arch-comb-low"]);
    assert_eq!(evidence.level, EvidenceLevel::DifferentiallyVerified);
    assert_eq!(evidence.sources, ["arch-comb-high", "arch-comb-low"]);
    assert!(evidence.is_production_ready());
}

#[test]
fn a_level_above_hypothesis_must_name_at_least_one_source() {
    // The rule that stops the type from being decoration. `Hypothesis`
    // is the one level that may stand alone, because "nothing
    // established this yet" is exactly what it means; every other level
    // is a claim that something did, and has to say what.
    assert!(Evidence::checked(EvidenceLevel::Hypothesis, &[]).is_ok());
    assert_eq!(
        Evidence::checked(EvidenceLevel::DatasheetSpecified, &[]),
        Err(EvidenceLevel::DatasheetSpecified)
    );
    assert!(Evidence::checked(EvidenceLevel::HardwareVerified, &["part-marking"]).is_ok());
}

#[test]
fn the_weakest_of_several_facts_is_what_a_conclusion_rests_on() {
    // A mapping assembled from several measurements is only as
    // established as its least-established part, and the arithmetic has
    // to say so rather than averaging — the ATF22V10's column map is
    // measured, but a conclusion drawn from it and from the
    // intact-means-connected convention is `DatasheetSpecified`,
    // because that convention is.
    let combined = Evidence::weakest([
        Evidence::new(EvidenceLevel::OpenSourceCrossChecked, &["galette"]),
        Evidence::new(EvidenceLevel::DatasheetSpecified, &["jedec-3a"]),
        Evidence::new(EvidenceLevel::DifferentiallyVerified, &["in2"]),
    ]);
    assert_eq!(combined.level, EvidenceLevel::DatasheetSpecified);
    // And it carries every source, so the weak link is findable.
    assert_eq!(combined.sources, ["galette", "jedec-3a", "in2"]);
}

#[test]
fn the_weakest_of_nothing_is_a_hypothesis_not_hardware_verification() {
    // Seeding a minimum with the strongest level and folding is the
    // natural way to write `weakest`, and it answers `HardwareVerified`
    // for an empty iterator — maximal evidence for no facts at all.
    //
    // On this project that is the worst possible default: a caller
    // combining an empty set of measurements would be told its
    // conclusion is hardware-verified, which is the one level SPEC.md
    // §7.8 requires a physical record to accompany.
    let nothing = Evidence::weakest([]);
    assert_eq!(nothing.level, EvidenceLevel::Hypothesis);
    assert!(nothing.sources.is_empty());
    assert!(!nothing.is_production_ready(), "nothing may not ship");
}
