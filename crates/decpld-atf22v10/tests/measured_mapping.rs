//! The measured ATF22V10C mapping, asserted against the numbers the
//! oracle produced.
//!
//! Every expectation here is transcribed from
//! `targets/evidence/atf22v10-fuse-map.md`, which cites the experiment
//! that produced it. If the oracle is later shown wrong, these are the
//! tests that trusted it and this is where to look.

use decpld_atf22v10::*;
use decpld_device::{FuseId, FuseMap, FuseMutability};

const G: Atf22v10Geometry = Atf22v10Geometry;

#[test]
fn input_pins_land_on_the_measured_columns() {
    // Experiments in1..in11, in13. Output fixed on pin 23, input swept;
    // exactly one column stays intact and that column is the answer.
    let expected: [(u8, u32); 12] = [
        (1, 0),
        (2, 4),
        (3, 8),
        (4, 12),
        (5, 16),
        (6, 20),
        (7, 24),
        (8, 28),
        (9, 32),
        (10, 36),
        (11, 40),
        (13, 42),
    ];
    for (pin, column) in expected {
        let source = G
            .sources()
            .find(|s| s.kind == SourceKind::Input(PinNumber(pin)))
            .unwrap_or_else(|| panic!("pin {pin} has no source"));
        assert_eq!(source.true_column(), column, "pin {pin}");
        assert_eq!(source.complement_column(), column + 1, "pin {pin} complement");
    }
}

#[test]
fn macrocell_feedback_lands_on_the_measured_columns() {
    // Experiments fb14..fb23. Pin n drives pin 23, so pin 23's data
    // term names pin n's feedback column.
    let expected: [(u8, u32); 10] = [
        (23, 2),
        (22, 6),
        (21, 10),
        (20, 14),
        (19, 18),
        (18, 22),
        (17, 26),
        (16, 30),
        (15, 34),
        (14, 38),
    ];
    for (pin, column) in expected {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let source = G
            .sources()
            .find(|s| s.kind == SourceKind::Feedback(macrocell))
            .unwrap_or_else(|| panic!("pin {pin} feedback has no source"));
        assert_eq!(source.true_column(), column, "pin {pin} feedback");
    }
}

#[test]
fn every_column_is_claimed_exactly_once() {
    // 22 sources x 2 senses = 44 columns, none shared and none missing.
    // A source silently sharing a column with another is a mapping
    // error that still produces a plausible file.
    let mut seen = vec![0u32; COLUMNS as usize];
    for source in G.sources() {
        seen[source.true_column() as usize] += 1;
        seen[source.complement_column() as usize] += 1;
    }
    assert_eq!(G.sources().count(), 22, "22 signal sources");
    for (column, count) in seen.iter().enumerate() {
        assert_eq!(*count, 1, "column {column} is claimed {count} times");
    }
}

#[test]
fn source_21_is_an_input_not_feedback() {
    // The exception that breaks "odd means feedback". It holds for all
    // ten feedbacks and still gets this one of eleven odd sources
    // wrong — extrapolating from pin 22 would have put a wrong column
    // in the model.
    let source = G.source(21).expect("source 21 exists");
    assert_eq!(source.kind, SourceKind::Input(PinNumber(13)));
    assert_eq!(source.true_column(), 42);
}

#[test]
fn row_blocks_start_where_they_were_measured() {
    // Measured at pins 23, 22, 20, 17, 14 — spanning the range rather
    // than clustered at one end.
    let expected: [(u8, u32); 5] = [(23, 1), (22, 10), (20, 34), (17, 83), (14, 122)];
    for (pin, first_row) in expected {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let block = G.row_block(macrocell).expect("a block");
        assert_eq!(block.output_enable_row, first_row, "pin {pin}");
    }
}

#[test]
fn architecture_pairs_run_opposite_to_row_blocks() {
    // The reversal, stated as the thing it is: j = 9 - i. The single
    // most likely place for this mapping to be silently transposed,
    // measured at the same five points as the row blocks.
    let expected: [(u8, u32); 5] = [(23, 5808), (22, 5810), (20, 5814), (17, 5820), (14, 5826)];
    for (pin, s0) in expected {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let pair = G.architecture_pair(macrocell).expect("a pair");
        assert_eq!(pair.polarity.0, s0, "pin {pin} S0");
        assert_eq!(pair.mode.0, s0 + 1, "pin {pin} S1");
    }

    // And the orderings genuinely disagree: pin 23 has the lowest rows
    // AND the lowest architecture pair, while carrying the HIGHEST
    // macrocell index. Two of the three run one way and one the other.
    let pin23 = G.macrocell_of_pin(PinNumber(23)).unwrap();
    let pin14 = G.macrocell_of_pin(PinNumber(14)).unwrap();
    assert!(
        G.row_block(pin23).unwrap().output_enable_row
            < G.row_block(pin14).unwrap().output_enable_row
    );
    assert!(
        G.architecture_pair(pin23).unwrap().polarity.0
            < G.architecture_pair(pin14).unwrap().polarity.0
    );
    assert!(pin23 > pin14, "and the macrocell indices run the other way");
}

#[test]
fn row_blocks_partition_the_array_leaving_only_the_two_control_rows() {
    // Blocks must not overlap and must account for 130 of the 132 rows.
    // The two left over are rows 0 and 131, which no experiment here
    // exercises — the evidence document says so rather than guessing
    // they are AR and SP.
    let mut used = vec![false; ROWS as usize];
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let block = G.row_block(MacrocellIndex(index)).expect("a block");
        for row in std::iter::once(block.output_enable_row).chain(block.data_rows.clone()) {
            assert!(!used[row as usize], "row {row} belongs to two macrocells");
            used[row as usize] = true;
        }
        let data = block.data_rows.end - block.data_rows.start;
        assert!((8..=16).contains(&data), "macrocell {index} has {data} data terms");
    }
    let unused: Vec<usize> =
        used.iter().enumerate().filter(|(_, u)| !**u).map(|(r, _)| r).collect();
    assert_eq!(unused, vec![0, 131], "only the two control rows are unaccounted for");
}

#[test]
fn the_three_footprints_have_the_measured_fuse_counts() {
    // Experiments mode-pal, arch-comb-high, mode-powerdown: one design
    // compiled under each WinCUPL device type.
    assert_eq!(Footprint::Pal.fuse_count(), 5828);
    assert_eq!(Footprint::Gal.fuse_count(), 5892);
    assert_eq!(Footprint::PowerDown.fuse_count(), 5893);

    for footprint in Footprint::ALL {
        assert_eq!(Footprint::from_fuse_count(footprint.fuse_count()), Ok(footprint));
    }
    assert!(Footprint::from_fuse_count(2194).is_err(), "a GAL16V8 count is not ours");
}

#[test]
fn an_unknown_fuse_count_names_the_three_that_are_accepted() {
    // A user arriving with the wrong device's file should be told what
    // this device does accept, not merely that theirs is wrong.
    let message = Footprint::from_fuse_count(5000).unwrap_err().to_string();
    for expected in ["5828", "5892", "5893"] {
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn every_footprint_classifies_every_fuse() {
    // FuseRegions enforces the partition, so this asserts each
    // footprint's regions are constructible — that the boundaries add up.
    for footprint in Footprint::ALL {
        let regions = regions_for(footprint).unwrap_or_else(|e| panic!("{footprint:?}: {e}"));
        assert_eq!(regions.count(), footprint.fuse_count(), "{footprint:?}");
    }
}

#[test]
fn pal_mode_has_no_signature_and_power_down_mode_has_the_extra_fuse() {
    // Measured: mode-pal yields QF5828 with nothing set above the
    // architecture block, and mode-powerdown is the GAL output plus
    // fuse 5892.
    let pal = regions_for(Footprint::Pal).unwrap();
    assert!(pal.iter().all(|r| r.mutability != FuseMutability::UserSignature));

    let gal = regions_for(Footprint::Gal).unwrap();
    let signature = gal
        .iter()
        .find(|r| r.mutability == FuseMutability::UserSignature)
        .expect("GAL mode has a signature");
    assert_eq!(signature.range, 5828..5892);

    let pd = regions_for(Footprint::PowerDown).unwrap();
    let power_down = pd.iter().find(|r| r.name == "power-down").expect("the extra fuse");
    assert_eq!(power_down.range, 5892..5893);
}

#[test]
fn the_security_fuse_is_not_in_the_fuse_map() {
    // The datasheet says the device has one, but it is the JEDEC `G`
    // field rather than a numbered fuse: every footprint's count is
    // fully accounted for without it. Modelling it here would mean
    // inventing an index with nothing to cite.
    for footprint in Footprint::ALL {
        let regions = regions_for(footprint).unwrap();
        assert!(
            regions.iter().all(|r| r.mutability != FuseMutability::Security),
            "{footprint:?} must not invent a security fuse index"
        );
        let mut map = FuseMap::erased(regions);
        assert!(map.set_security_fuse(true).is_err());
    }
}

#[test]
fn an_erased_device_has_every_array_link_blown() {
    // An erased GAL cell is a broken link, so every product term is the
    // empty AND — constantly true. Getting this backwards would invert
    // the meaning of every unprogrammed row.
    let map = FuseMap::erased(regions_for(Footprint::Gal).unwrap());
    for fuse in [0, 100, 5807] {
        assert_eq!(map.get(FuseId(fuse)), Some(true), "fuse {fuse}");
    }
}
