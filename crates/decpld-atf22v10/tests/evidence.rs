//! Every citation names something that exists, every mapping is
//! established well enough to ship, and the table agrees with the
//! document it projects. SPEC.md §13.1.

use decpld_atf22v10::Mapping;
use decpld_device::EvidenceLevel;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn repo_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(relative)
}

fn experiments_dir() -> PathBuf {
    repo_path("targets/experiments/atf22v10")
}

/// Every `<name>.pld` in the suite.
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

/// Every `id` registered in `references.toml`.
///
/// Parsed as keys rather than searched for as text. A substring test
/// against the whole file accepts any word that appears anywhere in it —
/// including inside a prose `notes` block — which makes "the datasheet
/// says so" checkable against nothing at all.
fn reference_ids() -> BTreeSet<String> {
    let path = repo_path("targets/evidence/references.toml");
    let text = std::fs::read_to_string(&path).expect("references.toml is in the repository");
    text.lines()
        .filter_map(|line| {
            let value = line.trim().strip_prefix("id")?.trim_start().strip_prefix('=')?.trim();
            value.strip_prefix('"')?.strip_suffix('"').map(str::to_owned)
        })
        .collect()
}

/// The experiments each section of the evidence document names, keyed by
/// section title.
///
/// A section owns its subsections: `## Pin roles` collects the
/// `Experiments:` lines of every `###` beneath it, which is where that
/// section's measurements actually live.
///
/// A citation is a backticked token spelled the way an experiment file
/// is: letters, digits and dashes, nothing else. That rule is what lets
/// an `Experiments:` line go on being prose — `EXPECT refusal` is a CUPL
/// marker and `.oe` is a CUPL keyword, and both appear backticked on
/// these lines. A token that looks like a name but names nothing still
/// fails, in the caller.
fn document_citations() -> BTreeMap<String, BTreeSet<String>> {
    let text = std::fs::read_to_string(repo_path("targets/evidence/atf22v10-fuse-map.md"))
        .expect("the evidence document is in the repository");

    let mut citations_by_section: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // The headings enclosing the current line, outermost first, so a
    // citation can be filed under every section that owns it.
    let mut open: Vec<(usize, String)> = Vec::new();

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('#') {
            let depth = 1 + rest.chars().take_while(|c| *c == '#').count();
            let title = rest.trim_start_matches('#').trim().to_owned();
            open.retain(|(held, _)| *held < depth);
            open.push((depth, title.clone()));
            citations_by_section.entry(title).or_default();
            continue;
        }

        let trimmed = line.trim_start();
        if !trimmed.starts_with("Experiments:") && !trimmed.starts_with("Experiment:") {
            continue;
        }
        let cited: BTreeSet<String> = trimmed
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|token| {
                !token.is_empty() && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
            .map(str::to_owned)
            .collect();
        for (_, section) in &open {
            citations_by_section.entry(section.clone()).or_default().extend(cited.iter().cloned());
        }
    }
    citations_by_section
}

/// The experiment names a mapping cites, dropping document references.
fn cited_experiments(mapping: Mapping) -> BTreeSet<String> {
    let documents = reference_ids();
    mapping
        .evidence()
        .sources
        .iter()
        .filter(|source| !documents.contains(**source))
        .map(|source| (*source).to_owned())
        .collect()
}

#[test]
fn the_experiment_index_lists_only_designs() {
    // `run.sh` lives in the same directory and is the batch runner, not
    // a measurement. Without the extension filter it would enter the
    // index as `run` and a citation could name it.
    let names = experiment_names();
    assert!(!names.contains("run"), "the runner is not an experiment");
    for name in &names {
        assert!(
            experiments_dir().join(format!("{name}.pld")).is_file(),
            "`{name}` is indexed but has no design file"
        );
    }
}

#[test]
fn every_cited_source_is_an_experiment_or_a_registered_reference() {
    // The reason this table is data rather than only a comment. A
    // comment citing `oe-var` goes on reading plausibly after the file
    // is renamed; this fails. It is also how a claim that trusted an
    // oracle run gets found if the run is later shown wrong — the
    // citation is the index.
    //
    // Documents and experiments are checked against their own registries
    // rather than against an allowlist held here: an allowlist is a hole
    // in exactly this test, since anything named in it is exempt from
    // having to exist.
    let experiments = experiment_names();
    let documents = reference_ids();
    for mapping in Mapping::ALL.iter().copied() {
        for source in mapping.evidence().sources {
            assert!(
                experiments.contains(*source) || documents.contains(*source),
                "{mapping:?} cites `{source}`, which is neither an experiment in {} \
                 nor a reference registered in targets/evidence/references.toml",
                experiments_dir().display()
            );
        }
    }
}

#[test]
fn the_table_cites_exactly_what_the_evidence_document_cites() {
    // The document is the argument; this table is a projection of it,
    // and the `Experiments:` lines are the interface. Checking both
    // directions is the point: dropping a citation here would leave the
    // document claiming ten measured macrocells while the table named
    // two, and adding one here would credit a measurement the argument
    // never made.
    let document = document_citations();
    let experiments = experiment_names();

    for mapping in Mapping::ALL.iter().copied() {
        let sections = mapping.document_sections();
        let mut expected = BTreeSet::new();
        for section in sections {
            let cited = document.get(*section).unwrap_or_else(|| {
                panic!(
                    "{mapping:?} names document section `{section}`, \
                     which is not a heading in targets/evidence/atf22v10-fuse-map.md"
                )
            });
            expected.extend(cited.iter().cloned());
        }
        for name in &expected {
            assert!(
                experiments.contains(name),
                "the evidence document cites `{name}` for {mapping:?}, which is not in the suite"
            );
        }
        assert_eq!(
            cited_experiments(mapping),
            expected,
            "{mapping:?} and its document sections {sections:?} disagree about which \
             experiments established it"
        );
    }
}

#[test]
fn a_mapping_resting_only_on_documents_cites_no_experiment() {
    // The escape hatch, closed. A mapping with no document section has
    // nothing for the projection above to check, so it must be a mapping
    // no experiment touched — which is exactly the state that has to be
    // visible rather than convenient.
    for mapping in Mapping::ALL.iter().copied() {
        if mapping.document_sections().is_empty() {
            assert!(
                cited_experiments(mapping).is_empty(),
                "{mapping:?} cites experiments but names no section of the evidence \
                 document, so nothing checks that the document agrees"
            );
        }
    }
}

#[test]
fn every_mapping_holds_the_level_the_evidence_document_argues_for() {
    // Pinned, because the levels are the part a wrong fuse comes from
    // and the machinery around them cannot tell whether a claim is
    // honest. Raising or lowering one has to change this table too, and
    // the argument for the new level belongs in the section named
    // beside it.
    use EvidenceLevel::{DatasheetSpecified, DifferentiallyVerified, OpenSourceCrossChecked};
    let expected = [
        // "Galette and GALasm both encode the array as row·44 + column".
        (Mapping::ArrayAddressing, OpenSourceCrossChecked),
        // WinCUPL is the only witness to the column map (§Evidence level).
        (Mapping::ColumnMap, DifferentiallyVerified),
        // Measured row starts, identical to Galette's `OLMC_ROWS_22V10`.
        (Mapping::RowBlocks, OpenSourceCrossChecked),
        // Three blocks filled and the term past each refused, agreeing
        // with `OLMC_SIZE_22V10`.
        (Mapping::CapacityMeasured, OpenSourceCrossChecked),
        // The other seven sizes: two documents agreeing, no experiment.
        // Not `OpenSourceCrossChecked`, because the ladder is a total
        // order and that rung asserts a differential nothing ran.
        (Mapping::CapacityCrossChecked, DatasheetSpecified),
        // S0/S1 semantics and the reversed pair order are WinCUPL-only;
        // cross-checking surfaced the ordering as a discrepancy and
        // could not settle it.
        (Mapping::ArchitectureBits, DifferentiallyVerified),
        (Mapping::OutputEnable, DifferentiallyVerified),
        // Which rail is which is a one-witness claim no fuse experiment
        // can reach, and the entry spans it.
        (Mapping::PinRoles, DatasheetSpecified),
        (Mapping::Footprints, DifferentiallyVerified),
        (Mapping::UserSignature, DifferentiallyVerified),
        // Only hardware can corroborate it. See below.
        (Mapping::LinkConvention, DatasheetSpecified),
    ];
    assert_eq!(expected.len(), Mapping::ALL.len(), "a mapping is missing from this table");
    for (mapping, level) in expected {
        assert_eq!(mapping.evidence().level, level, "{mapping:?}");
    }
}

#[test]
fn every_mapping_meets_the_production_threshold() {
    // CLAUDE.md: unverified hypotheses belong in oracle-analysis code
    // or disabled experimental targets, never in a production target.
    // This is that rule, checkable.
    for mapping in Mapping::ALL.iter().copied() {
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
fn capacity_overall_is_only_as_established_as_its_weakest_block() {
    // Splitting the mapping is what lets the measured blocks keep their
    // level without lending it to the seven that were never filled.
    // Anything reasoning about "capacity" as one fact has to combine
    // them, and the combination is a minimum.
    let combined = decpld_device::Evidence::weakest([
        Mapping::CapacityMeasured.evidence(),
        Mapping::CapacityCrossChecked.evidence(),
    ]);
    assert_eq!(combined.level, EvidenceLevel::DatasheetSpecified);
    assert!(combined.sources.contains(&"cap23-9"), "the weak link stays findable");
    assert!(combined.sources.contains(&"galette"));
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
        Mapping::ALL.iter().map(|mapping| mapping.evidence().level).min(),
        Some(EvidenceLevel::DatasheetSpecified)
    );
}

#[test]
fn nothing_on_this_device_is_hardware_verified_yet() {
    // The honest state of the project, asserted so that it cannot drift
    // upward by wishful editing. A mapping claiming `HardwareVerified`
    // must arrive with SPEC.md §7.8's record: part marking, programmer
    // version, JEDEC hash, vectors, results.
    for mapping in Mapping::ALL.iter().copied() {
        assert_ne!(
            mapping.evidence().level,
            EvidenceLevel::HardwareVerified,
            "{mapping:?} claims hardware verification; §7.8 requires the record to go with it"
        );
    }
}
