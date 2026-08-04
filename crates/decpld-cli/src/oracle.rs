//! `decpld oracle diff` — a fuse diff read as device behaviour.
//!
//! SPEC.md §7.5. `jed diff` already compares two files by fuse vector
//! rather than by text, which is what makes reverse-engineering
//! possible at all: change one thing in a `.pld` and see which *fuses*
//! moved. This adds the second half. "Fuse 52 changed" is not a
//! finding; "macrocell 9's output-enable term gained pin 3, true sense"
//! is, and the difference between those two sentences is a day's work
//! reading tables.
//!
//! Everything here is a pure function of two parsed files, so it is
//! tested without spawning a process.

use decpld_atf22v10::Footprint;
use decpld_device::{FuseId, FuseMeaning, classify_fuse};
use decpld_jedec::{JedecDiff, JedecFile};
use std::collections::BTreeMap;

use crate::inspect::{Device, InspectError};

/// One fuse that moved, and what it controls.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedDelta {
    pub fuse: FuseId,
    pub before: bool,
    pub after: bool,
    pub meaning: FuseMeaning,
}

/// A whole diff, with every fuse delta explained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassifiedDiff {
    pub device: &'static str,
    /// Which of the device's JEDEC footprints both files use.
    pub footprint: Footprint,
    pub deltas: Vec<ClassifiedDelta>,
    /// How many deltas fell into each category, for the one-line shape
    /// of a run before reading it in detail.
    pub counts_by_category: BTreeMap<&'static str, usize>,
    /// Carried through unchanged: these are facts about the *files*,
    /// and a device model has nothing to add to them.
    pub file: JedecDiff,
}

impl ClassifiedDiff {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty() && self.file.is_empty()
    }
}

/// Compare two files and explain every fuse that moved.
///
/// # Errors
///
/// If either file's fuse count is not one the named device has. Both
/// are checked, not just the first: comparing an ATF22V10C against
/// something else and classifying the result against the ATF22V10C's
/// map would describe the second file as a device it is not.
pub fn diff(
    before: &JedecFile,
    after: &JedecFile,
    device: Device,
) -> Result<ClassifiedDiff, InspectError> {
    // Both counts are checked, not just the first: comparing an
    // ATF22V10C against something else and classifying the result
    // against the ATF22V10C map would describe the second file as a
    // device it is not.
    let model = device.model_for(before.fuses.len())?;
    model.check_fuse_count(after.fuses.len())?;

    let file = decpld_jedec::diff(before, after);

    let mut deltas = Vec::with_capacity(file.fuses.len());
    let mut counts_by_category: BTreeMap<&'static str, usize> = BTreeMap::new();
    for delta in &file.fuses {
        let meaning =
            classify_fuse(FuseId(delta.index), &model.matrix, &model.specs, &model.regions);
        *counts_by_category.entry(meaning.category()).or_default() += 1;
        deltas.push(ClassifiedDelta {
            fuse: FuseId(delta.index),
            before: delta.before,
            after: delta.after,
            meaning,
        });
    }

    Ok(ClassifiedDiff {
        device: model.device,
        footprint: model.footprint,
        deltas,
        counts_by_category,
        file,
    })
}

/// Render a classified diff. Pure, so the layout is testable without
/// files or a process.
#[must_use]
pub fn render(diff: &ClassifiedDiff, before: &str, after: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "--- {before}\n+++ {after}");
    let _ = writeln!(out, "device: {} ({:?} footprint)", diff.device, diff.footprint);

    if let Some((a, b)) = diff.file.fuse_count {
        // Cannot happen through `diff` above, which refuses a count the
        // device does not have — but `ClassifiedDiff` is public and a
        // renderer that silently omitted the one field that suppresses
        // every fuse comparison would be lying by omission.
        let _ = writeln!(out, "fuse count: {a} -> {b} (fuse states not compared)");
    }
    if let Some((a, b)) = &diff.file.design_specification {
        // Summarised, never printed. WinCUPL's header is a banner
        // carrying a timestamp, the design's name, and the
        // installation's SERIAL NUMBER — so it differs between any two
        // runs, would bury the fuse findings under a dozen lines of
        // noise, and is the one thing in an oracle result that must not
        // be redistributed (CLAUDE.md). None of it is a device fact.
        // `jed diff` prints the text for anyone who wants it.
        let changed = a.lines().zip(b.lines()).filter(|(x, y)| x != y).count()
            + a.lines().count().abs_diff(b.lines().count());
        let _ = writeln!(
            out,
            "design specification: {changed} line(s) differ (not shown: banner, timestamp, serial)"
        );
    }
    if let Some((a, b)) = &diff.file.notes {
        let _ = writeln!(out, "notes: {a:?} -> {b:?}");
    }
    if let Some((a, b)) = &diff.file.unknown_fields {
        let _ = writeln!(out, "unmodelled fields: {a:?} -> {b:?}");
    }
    if let Some((a, b)) = diff.file.security {
        let _ = writeln!(out, "security fuse: {a:?} -> {b:?}");
    }

    if diff.deltas.is_empty() {
        let _ = writeln!(out, "\nno fuse changed");
        return out;
    }

    let _ = writeln!(out, "\n{} fuse(s) changed:", diff.deltas.len());
    for (category, count) in &diff.counts_by_category {
        let _ = writeln!(out, "  {count:>5}  {category}");
    }

    // Grouped by category rather than listed in fuse order. A single
    // varied literal produces two deltas in one category, while a
    // changed macrocell produces dozens across several — and which
    // groups appear at all is the finding an experiment is looking for.
    let _ = writeln!(out);
    let mut current: Option<&'static str> = None;
    for delta in ordered_by_category(diff) {
        if current != Some(delta.meaning.category()) {
            current = Some(delta.meaning.category());
            let _ = writeln!(out, "{}:", delta.meaning.category());
        }
        let _ = writeln!(
            out,
            "  {} {} -> {}   {}",
            delta.fuse,
            u8::from(delta.before),
            u8::from(delta.after),
            delta.meaning
        );
    }
    out
}

/// Deltas grouped by category, ascending by fuse within each.
///
/// A stable order: `counts_by_category` is a `BTreeMap`, and the deltas
/// arrive in fuse order from `JedecDiff`, so two runs over one pair of
/// files print identical bytes (SPEC.md §13.2).
fn ordered_by_category(diff: &ClassifiedDiff) -> impl Iterator<Item = &ClassifiedDelta> {
    diff.counts_by_category.keys().flat_map(|category| {
        diff.deltas.iter().filter(move |delta| delta.meaning.category() == *category)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use decpld_atf22v10::{Atf22v10Geometry, blank_design, bool_input_of_source, encode_design};
    use decpld_device::{
        FuseMap, MacrocellConfig, MacrocellId, MacrocellMode, OutputPolarity, PinNumber,
        PlacedCube, ProductTermId,
    };
    use decpld_jedec::FuseVector;
    use decpld_logic::{Cube, Literal, Polarity};

    const G: Atf22v10Geometry = Atf22v10Geometry;

    fn literal(pin: u8, polarity: Polarity) -> Literal {
        let source = G.source_of_pin(PinNumber(pin)).expect("a signal pin");
        Literal::new(bool_input_of_source(source.index), polarity)
    }

    fn cell(design: &mut decpld_device::PhysicalDesign, pin: u8) -> &mut MacrocellConfig {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        design
            .macrocells
            .iter_mut()
            .find(|cell| cell.id == MacrocellId(macrocell.0))
            .expect("present")
    }

    /// `in2`: pin 2 drives pin 23, always enabled.
    fn in2() -> JedecFile {
        file_of(&encode(|design| {
            let block = G
                .row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin"))
                .expect("a block");
            let cell = cell(design, 23);
            cell.oe_term = Some(PlacedCube {
                row: ProductTermId(block.output_enable_row),
                cube: Cube::always(),
            });
            cell.data_terms = vec![PlacedCube {
                row: ProductTermId(block.data_rows.start),
                cube: Cube::new([literal(2, Polarity::True)]),
            }];
        }))
    }

    fn encode(build: impl FnOnce(&mut decpld_device::PhysicalDesign)) -> FuseMap {
        let mut design = blank_design().expect("a blank design");
        build(&mut design);
        encode_design(&design, Footprint::Gal).expect("encodable")
    }

    fn file_of(fuses: &FuseMap) -> JedecFile {
        let mut vector = FuseVector::new(fuses.len(), false);
        for (index, state) in fuses.iter().enumerate() {
            let index = u32::try_from(index).expect("under six thousand");
            vector.set(index, state).expect("in range");
        }
        JedecFile {
            design_specification: "probe\n".to_owned(),
            fuses: vector,
            default_fuse: Some(false),
            notes: Vec::new(),
            security: None,
            fuse_checksum: None,
            transmission_checksum: None,
            unknown_fields: Vec::new(),
        }
    }

    #[test]
    fn one_varied_literal_produces_one_delta_named_by_its_resource() {
        // The `oe-var` experiment, done in-process. `jed diff` can say
        // "fuse 52: 1 -> 0"; only this can say which product term of
        // which macrocell gained which pin, and that sentence is the
        // whole point of `--device`.
        let block =
            G.row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin")).expect("a block");
        let gated = file_of(&encode(|design| {
            let cell = cell(design, 23);
            cell.oe_term = Some(PlacedCube {
                row: ProductTermId(block.output_enable_row),
                cube: Cube::new([literal(3, Polarity::True)]),
            });
            cell.data_terms = vec![PlacedCube {
                row: ProductTermId(block.data_rows.start),
                cube: Cube::new([literal(2, Polarity::True)]),
            }];
        }));

        let delta = diff(&in2(), &gated, Device::Atf22v10c).expect("both are this device");
        assert_eq!(delta.deltas.len(), 1);
        let one = &delta.deltas[0];
        assert_eq!(one.fuse, FuseId(52), "pin 3's true column in the enable row");
        assert_eq!((one.before, one.after), (true, false));
        assert_eq!(one.meaning.category(), "matrix-output-enable");
        assert_eq!(
            one.meaning.to_string(),
            "macrocell 9 output-enable term in product term 1: literal pin 3"
        );
        assert_eq!(delta.counts_by_category, [("matrix-output-enable", 1)].into_iter().collect());
    }

    #[test]
    fn turning_a_pad_off_moves_one_whole_row_and_nothing_else() {
        // The `oe-never` measurement. Forty-four deltas, every one of
        // them in the same output-enable row — which is what makes it a
        // clean differential, and exactly what the category summary is
        // for: one line saying so instead of forty-four to read.
        let off = file_of(&encode(|design| {
            let block = G
                .row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin"))
                .expect("a block");
            let cell = cell(design, 23);
            cell.oe_term = None;
            cell.data_terms = vec![PlacedCube {
                row: ProductTermId(block.data_rows.start),
                cube: Cube::new([literal(2, Polarity::True)]),
            }];
        }));

        let delta = diff(&in2(), &off, Device::Atf22v10c).expect("both are this device");
        assert_eq!(delta.deltas.len(), 44);
        assert_eq!(delta.counts_by_category, [("matrix-output-enable", 44)].into_iter().collect());
        assert!(delta.deltas.iter().all(|d| (d.before, d.after) == (true, false)));
        assert_eq!(delta.deltas.first().map(|d| d.fuse), Some(FuseId(44)));
        assert_eq!(delta.deltas.last().map(|d| d.fuse), Some(FuseId(87)));
    }

    #[test]
    fn a_changed_macrocell_configuration_is_separated_from_changed_logic() {
        // A design that changes both mode and a literal produces deltas
        // in three categories, and telling them apart is the
        // classification's job — a run reported as "45 fuses changed"
        // says nothing about which of the two edits did what.
        let block =
            G.row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin")).expect("a block");
        let changed = file_of(&encode(|design| {
            let cell = cell(design, 23);
            cell.mode = MacrocellMode::Registered;
            cell.polarity = OutputPolarity::ActiveLow;
            cell.oe_term = Some(PlacedCube {
                row: ProductTermId(block.output_enable_row),
                cube: Cube::always(),
            });
            cell.data_terms = vec![PlacedCube {
                row: ProductTermId(block.data_rows.start),
                cube: Cube::new([literal(4, Polarity::True)]),
            }];
        }));

        let delta = diff(&in2(), &changed, Device::Atf22v10c).expect("both are this device");
        assert_eq!(
            delta.counts_by_category,
            [("matrix-data", 2), ("mode", 1), ("polarity", 1)].into_iter().collect(),
            "the literal moved and both architecture bits cleared"
        );
    }

    #[test]
    fn identical_files_report_no_difference() {
        let delta = diff(&in2(), &in2(), Device::Atf22v10c).expect("this device");
        assert!(delta.is_empty());
        assert!(delta.deltas.is_empty());
    }

    #[test]
    fn both_files_are_checked_against_the_named_device_not_just_the_first() {
        // Classifying a foreign file's fuses against this device's map
        // would describe it as a part it is not — and the second
        // argument is exactly where that is easy to miss.
        let mut foreign = in2();
        foreign.fuses = FuseVector::new(2194, false);

        assert!(diff(&in2(), &foreign, Device::Atf22v10c).is_err(), "the second file");
        assert!(diff(&foreign, &in2(), Device::Atf22v10c).is_err(), "the first file");
    }

    #[test]
    fn the_rendered_report_groups_by_category_and_never_prints_the_header() {
        // WinCUPL's header carries a timestamp, the design's name, and
        // the installation's serial number. It differs between any two
        // runs, would bury the findings, and is the one part of an
        // oracle result that must not be redistributed.
        let mut before = in2();
        let mut after = in2();
        before.design_specification = "CUPL(WM) 5.0a Serial# MW-10400000\nName in2\n".to_owned();
        after.design_specification = "CUPL(WM) 5.0a Serial# MW-10400000\nName oe-var\n".to_owned();

        let delta = diff(&before, &after, Device::Atf22v10c).expect("this device");
        let text = render(&delta, "a.jed", "b.jed");
        assert!(!text.contains("MW-10400000"), "the serial must not be echoed:\n{text}");
        assert!(!text.contains("Name in2"), "nor the header text:\n{text}");
        assert!(text.contains("design specification: 1 line(s) differ"), "{text}");
        assert!(text.contains("no fuse changed"), "{text}");
    }

    #[test]
    fn rendering_is_deterministic() {
        // SPEC.md §13.2. Oracle work is comparative: two runs over one
        // pair of files that printed different bytes would make a
        // re-check look like a disagreement.
        let block =
            G.row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin")).expect("a block");
        let changed = file_of(&encode(|design| {
            let cell = cell(design, 23);
            cell.mode = MacrocellMode::Registered;
            cell.oe_term = Some(PlacedCube {
                row: ProductTermId(block.output_enable_row),
                cube: Cube::always(),
            });
            cell.data_terms = vec![PlacedCube {
                row: ProductTermId(block.data_rows.start),
                cube: Cube::new([literal(4, Polarity::True)]),
            }];
        }));
        let a = render(&diff(&in2(), &changed, Device::Atf22v10c).expect("ok"), "a", "b");
        let b = render(&diff(&in2(), &changed, Device::Atf22v10c).expect("ok"), "a", "b");
        assert_eq!(a, b);

        // Every delta of a category is listed under that category's
        // heading, and the headings are in the summary's order.
        let categories: Vec<&str> = a
            .lines()
            .filter(|line| line.ends_with(':') && !line.starts_with(' ') && !line.contains(' '))
            .collect();
        assert_eq!(categories, ["matrix-data:", "mode:"]);
    }
}
