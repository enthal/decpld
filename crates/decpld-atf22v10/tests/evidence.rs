//! Every citation names something that exists, and every mapping is
//! established well enough to ship. SPEC.md §13.1.

use decpld_atf22v10::Mapping;
use decpld_device::EvidenceLevel;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn experiments_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../targets/experiments/atf22v10")
}

fn experiment_names() -> BTreeSet<String> {
    std::fs::read_dir(experiments_dir())
        .expect("the experiment suite is in the repository")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "pld")
                .then(|| path.file_stem()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect()
}

/// References that are documents rather than experiments.
const DOCUMENTS: [&str; 1] = ["jedec-3a"];

#[test]
fn every_cited_experiment_exists() {
    // The reason this table is data rather than only a comment. A
    // comment citing `oe-var` goes on reading plausibly after the file
    // is renamed; this fails. It is also how a claim that trusted an
    // oracle run gets found if the run is later shown wrong — the
    // citation is the index.
    let available = experiment_names();
    for mapping in Mapping::ALL {
        for source in mapping.evidence().sources {
            if DOCUMENTS.contains(source) {
                continue;
            }
            assert!(
                available.contains(*source),
                "{mapping:?} cites experiment `{source}`, which is not in {}",
                experiments_dir().display()
            );
        }
    }
}

#[test]
fn every_document_reference_is_registered() {
    // A document citation has to be resolvable too. `references.toml`
    // records the exact revision and hash of each one, which is what
    // makes "the datasheet says so" checkable rather than a gesture.
    let references =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../targets/evidence/references.toml");
    let text = std::fs::read_to_string(&references).expect("references.toml is in the repository");
    for document in DOCUMENTS {
        assert!(text.contains(document), "`{document}` is not registered in {references:?}");
    }
}

#[test]
fn every_mapping_meets_the_production_threshold() {
    // CLAUDE.md: unverified hypotheses belong in oracle-analysis code
    // or disabled experimental targets, never in a production target.
    // This is that rule, checkable.
    for mapping in Mapping::ALL {
        let evidence = mapping.evidence();
        assert!(
            evidence.is_production_ready(),
            "{mapping:?} is only {} — it may not ship in a production target",
            evidence.level
        );
        assert!(!evidence.sources.is_empty(), "{mapping:?} names no source");
    }
}

#[test]
fn the_link_convention_is_the_weakest_thing_this_device_rests_on() {
    // Named explicitly, because it is the single most consequential bit
    // in the project: inverting it computes the complement of every
    // design behind a perfectly valid checksum. Every experiment is
    // CONSISTENT with it and none corroborates it — the reader and the
    // encoder share the convention, so a world where both are inverted
    // produces identical observations. Only hardware settles it.
    //
    // If this assertion ever fails because the level went UP, that is
    // good news that must be accompanied by the hardware run that
    // earned it.
    let convention = Mapping::LinkConvention.evidence();
    assert_eq!(convention.level, EvidenceLevel::DatasheetSpecified);
    assert_eq!(
        Mapping::ALL.into_iter().map(|m| m.evidence().level).min(),
        Some(EvidenceLevel::DatasheetSpecified)
    );
}

#[test]
fn nothing_on_this_device_is_hardware_verified_yet() {
    // The honest state of the project, asserted so that it cannot drift
    // upward by wishful editing. A mapping claiming `HardwareVerified`
    // must arrive with SPEC.md §7.8's record: part marking, programmer
    // version, JEDEC hash, vectors, results.
    for mapping in Mapping::ALL {
        assert_ne!(
            mapping.evidence().level,
            EvidenceLevel::HardwareVerified,
            "{mapping:?} claims hardware verification; §7.8 requires the record to go with it"
        );
    }
}
