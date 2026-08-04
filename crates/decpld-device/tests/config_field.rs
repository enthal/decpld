//! Configuration fields: a device option encoded as fuses. SPEC.md §4.3.
//!
//! The rules, over synthetic fields. The ATF22V10's own polarity and
//! mode fields are checked against measurements in `decpld-atf22v10`.

use decpld_device::{
    ConfigField, ConfigFieldError, ConfigFieldId, FuseId, FuseMap, FuseMutability, FuseRegion,
    FuseRegions, MacrocellMode, OutputPolarity,
};

fn erased(count: u32) -> FuseMap {
    let regions = FuseRegions::new(
        count,
        vec![FuseRegion {
            name: "architecture",
            range: 0..count,
            erased_value: true,
            mutability: FuseMutability::Programmable,
        }],
    )
    .expect("valid");
    FuseMap::erased(regions)
}

/// A one-bit polarity field: `1` is active high.
fn polarity_field() -> ConfigField<OutputPolarity> {
    ConfigField::new(
        ConfigFieldId(0),
        "polarity",
        vec![FuseId(0)],
        [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveLow, vec![false])],
    )
    .expect("a valid one-bit field")
}

#[test]
fn a_field_encodes_and_decodes_every_value_it_declares() {
    let field = polarity_field();
    for value in [OutputPolarity::ActiveHigh, OutputPolarity::ActiveLow] {
        let mut map = erased(2);
        field.encode(&mut map, value).expect("encodable");
        assert_eq!(field.decode(&map), Ok(value));
    }
}

#[test]
fn every_bit_pattern_decodes_to_something() {
    // A device model must be able to read back a part it did not
    // program. A field whose encoding covers only some patterns would
    // leave `jed inspect` unable to describe a real chip, so
    // construction requires totality rather than discovering the gap on
    // a user's file.
    let missing = ConfigField::new(
        ConfigFieldId(0),
        "two-bit",
        vec![FuseId(0), FuseId(1)],
        [(MacrocellMode::Combinational, vec![false, false])],
    );
    assert_eq!(
        missing,
        // Patterns are little-endian over `bits`: index 0 is the first
        // fuse. The first unassigned pattern is therefore [true, false],
        // not [false, true] — and naming the pattern rather than
        // counting a shortfall is what makes that legible.
        Err(ConfigFieldError::PatternUnassigned { name: "two-bit", pattern: vec![true, false] })
    );
}

#[test]
fn one_value_given_two_patterns_is_refused() {
    // The mirror of the case below, and the one that had no check.
    // Accepting it was last-wins, which broke totality from the far
    // side: the field validated as total, and then `decode` could not
    // name a state the chip can hold — the exact failure the totality
    // rule exists to prevent. It also made the declaration order of the
    // encoding list observable in the fuses.
    let twice = ConfigField::new(
        ConfigFieldId(0),
        "polarity",
        vec![FuseId(0)],
        [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveHigh, vec![false])],
    );
    assert_eq!(twice, Err(ConfigFieldError::ValueUsedTwice { name: "polarity" }));
}

#[test]
fn two_values_sharing_a_pattern_are_refused() {
    // Decoding would have to pick one, and picking is guessing about
    // what a chip actually does.
    let ambiguous = ConfigField::new(
        ConfigFieldId(0),
        "ambiguous",
        vec![FuseId(0)],
        [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveLow, vec![true])],
    );
    assert_eq!(
        ambiguous,
        Err(ConfigFieldError::PatternUsedTwice { name: "ambiguous", pattern: vec![true] })
    );
}

#[test]
fn a_pattern_that_does_not_match_the_bit_count_is_refused() {
    let wrong = ConfigField::new(
        ConfigFieldId(0),
        "wrong",
        vec![FuseId(0), FuseId(1)],
        [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveLow, vec![false, false])],
    );
    assert_eq!(
        wrong,
        Err(ConfigFieldError::PatternWidthMismatch { name: "wrong", bits: 2, pattern: 1 })
    );
}

#[test]
fn a_field_wider_than_a_pattern_index_is_refused() {
    // The ceiling is load-bearing for correctness, not termination: with
    // overflow checks off, `1usize << usize::BITS` masks to 1, so the
    // totality loop would examine one pattern and accept a field that is
    // not total. The diagnostic states the real limit rather than a
    // pattern width nobody supplied.
    let max = usize::BITS as usize - 1;
    let bits: Vec<FuseId> = (0..=max as u32).map(FuseId).collect();
    assert_eq!(
        ConfigField::<OutputPolarity>::new(ConfigFieldId(0), "wide", bits, []),
        Err(ConfigFieldError::FieldTooWide { name: "wide", bits: max + 1, max })
    );
}

#[test]
fn a_field_with_no_bits_or_a_repeated_bit_is_refused() {
    assert_eq!(
        ConfigField::<OutputPolarity>::new(ConfigFieldId(0), "empty", Vec::new(), []),
        Err(ConfigFieldError::NoBits { name: "empty" })
    );
    assert_eq!(
        ConfigField::new(
            ConfigFieldId(0),
            "repeat",
            vec![FuseId(0), FuseId(0)],
            [
                (OutputPolarity::ActiveHigh, vec![true, true]),
                (OutputPolarity::ActiveLow, vec![false, false]),
            ],
        ),
        Err(ConfigFieldError::FuseUsedTwice { name: "repeat", fuse: FuseId(0) })
    );
}

#[test]
fn encoding_a_value_the_field_does_not_declare_is_refused() {
    // `MacrocellMode` has three variants but this device's field encodes
    // two. Asking for the third must be an error, not a silent
    // substitution — an input-only macrocell that quietly became a
    // combinational output would drive a pin the design meant to leave
    // floating.
    let field = ConfigField::new(
        ConfigFieldId(1),
        "mode",
        vec![FuseId(0)],
        [(MacrocellMode::Combinational, vec![true]), (MacrocellMode::Registered, vec![false])],
    )
    .expect("valid");

    let mut map = erased(2);
    assert_eq!(
        field.encode(&mut map, MacrocellMode::InputOnly),
        // The id is carried because ten macrocells share the name
        // "mode"; a message naming only the kind of field says which
        // kind failed, not which one.
        Err(ConfigFieldError::ValueNotEncodable { name: "mode", id: ConfigFieldId(1) })
    );
    assert!(!map.is_written(FuseId(0)), "a refused encode must write nothing");
}

#[test]
fn encoding_then_decoding_round_trips_through_a_shared_fuse_map() {
    // Two fields on disjoint fuses of one map, which is how a macrocell
    // actually stores its polarity and mode.
    let polarity = polarity_field();
    let mode = ConfigField::new(
        ConfigFieldId(1),
        "mode",
        vec![FuseId(1)],
        [(MacrocellMode::Combinational, vec![true]), (MacrocellMode::Registered, vec![false])],
    )
    .expect("valid");

    for want_polarity in [OutputPolarity::ActiveHigh, OutputPolarity::ActiveLow] {
        for want_mode in [MacrocellMode::Combinational, MacrocellMode::Registered] {
            let mut map = erased(2);
            polarity.encode(&mut map, want_polarity).expect("encodable");
            mode.encode(&mut map, want_mode).expect("encodable");
            assert_eq!(polarity.decode(&map), Ok(want_polarity));
            assert_eq!(mode.decode(&map), Ok(want_mode));
        }
    }
}

#[test]
fn decoding_a_fuse_outside_the_map_names_the_fuse() {
    let field = polarity_field();
    let map = erased(1);
    let far = ConfigField::new(
        ConfigFieldId(2),
        "far",
        vec![FuseId(99)],
        [(OutputPolarity::ActiveHigh, vec![true]), (OutputPolarity::ActiveLow, vec![false])],
    )
    .expect("valid");
    // An erased architecture fuse reads `true`, which this field
    // defines as active high — the erased state of a real part, not a
    // neutral one.
    assert_eq!(field.decode(&map), Ok(OutputPolarity::ActiveHigh));
    assert_eq!(
        far.decode(&map),
        Err(ConfigFieldError::FuseOutOfRange {
            name: "far",
            id: ConfigFieldId(2),
            fuse: FuseId(99)
        })
    );
}
