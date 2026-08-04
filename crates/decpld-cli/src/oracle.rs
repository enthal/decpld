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
    /// Carried through unchanged: these are facts about the *files*,
    /// and a device model has nothing to add to them.
    pub file: JedecDiff,
}

impl ClassifiedDiff {
    /// Nothing to report at all.
    ///
    /// `file.is_empty()` alone would do — `deltas` is one-to-one with
    /// `file.fuses` — but the deltas are what this type is *for*, and a
    /// future constructor that populated them separately should not
    /// silently report nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.deltas.is_empty() && self.file.is_empty()
    }

    /// How many deltas fell into each category.
    ///
    /// Derived on demand rather than stored beside `deltas`. As a field
    /// the two could disagree, and `render` walks the categories to
    /// group its output — so a category missing from a stale map would
    /// drop its deltas from the body while the header still counted
    /// them, printing "3 fuse(s) changed" above two lines.
    #[must_use]
    pub fn counts_by_category(&self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for delta in &self.deltas {
            *counts.entry(delta.meaning.category()).or_default() += 1;
        }
        counts
    }
}

/// Compare two files and explain every fuse that moved.
///
/// # Errors
///
/// If either file's fuse count is not one the named device has, or if
/// the two are different footprints of it. Both checks matter: the
/// first stops a foreign file being classified against this device's
/// map, and the second stops a PAL-mode file being compared with a
/// GAL-mode one — across footprints not one fuse is comparable, and a
/// reader would see that as "no fuse changed".
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
    model.require_same_footprint(after.fuses.len())?;

    let file = decpld_jedec::diff(before, after);

    let mut deltas = Vec::with_capacity(file.fuses.len());
    for delta in &file.fuses {
        let meaning =
            classify_fuse(FuseId(delta.index), &model.matrix, &model.specs, &model.regions);
        deltas.push(ClassifiedDelta {
            fuse: FuseId(delta.index),
            before: delta.before,
            after: delta.after,
            meaning,
        });
    }

    Ok(ClassifiedDiff { device: model.device, footprint: model.footprint, deltas, file })
}

/// Render a classified diff. Pure, so the layout is testable without
/// files or a process.
#[must_use]
pub fn render(diff: &ClassifiedDiff, before: &str, after: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "--- {before}\n+++ {after}");
    let _ = writeln!(out, "device: {} ({})", diff.device, diff.footprint);

    if let Some((a, b)) = diff.file.fuse_count {
        // Unreachable through `diff` above, which now refuses two files
        // of different footprints before this point — that refusal is
        // the whole fix for a cross-footprint comparison reporting "no
        // fuse changed". It is still rendered rather than dropped:
        // `ClassifiedDiff` is public data, and a renderer that silently
        // omitted the one field suppressing every fuse comparison would
        // lie by omission about a value somebody built by hand.
        let _ = writeln!(out, "fuse count: {a} -> {b} (fuse states not compared)");
    }
    // NONE of the free-text fields are printed, only counted.
    //
    // WinCUPL's banner carries a timestamp, the design's name, and the
    // installation's SERIAL NUMBER, and CLAUDE.md forbids
    // redistributing that. It is not only the header: a `.pld` author
    // can put anything in an `N` note, and an unmodelled field is
    // reproduced identifier *and body*, so a serial reaches stdout
    // through either. `oracle diff` output is exactly what gets pasted
    // into an evidence file, so the rule has to hold for every text
    // field rather than the one it was first noticed on.
    //
    // None of it is a device fact in any case. `jed diff` prints the
    // text for anyone who wants it.
    if let Some((a, b)) = &diff.file.design_specification {
        let _ = writeln!(out, "design specification: {}", line_summary(a, b));
    }
    if let Some((a, b)) = &diff.file.notes {
        let _ = writeln!(out, "notes: {} before, {} after (text not shown)", a.len(), b.len());
    }
    if let Some((a, b)) = &diff.file.unknown_fields {
        // Identifiers only. `QP24` says a field changed and which kind;
        // its body is the part that can carry anything.
        let _ = writeln!(
            out,
            "unmodelled fields: {} -> {} (identifiers only)",
            identifiers(a),
            identifiers(b)
        );
    }
    if let Some((a, b)) = diff.file.security {
        let _ =
            writeln!(out, "security fuse: {} -> {}", describe_security(a), describe_security(b));
    }

    if diff.deltas.is_empty() {
        let _ = writeln!(out, "\nno fuse changed");
        return out;
    }

    let counts = diff.counts_by_category();
    let _ = writeln!(out, "\n{} fuse(s) changed:", diff.deltas.len());
    for (category, count) in &counts {
        let _ = writeln!(out, "  {count:>5}  {category}");
    }

    // Grouped by category rather than listed in fuse order. A single
    // varied literal produces two deltas in one category, while a
    // changed macrocell produces dozens across several — and which
    // groups appear at all is the finding an experiment is looking for.
    let _ = writeln!(out);
    let mut current: Option<&'static str> = None;
    for delta in ordered_by_category(diff, &counts) {
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
/// A stable order: the categories come from a `BTreeMap`, and the
/// deltas arrive in fuse order from `JedecDiff`, so two runs over one
/// pair of files print identical bytes (SPEC.md §13.2).
///
/// Takes the counts the caller already computed rather than recomputing
/// them, so the headings and the body cannot come from two different
/// walks of `deltas`.
fn ordered_by_category<'a>(
    diff: &'a ClassifiedDiff,
    counts: &'a BTreeMap<&'static str, usize>,
) -> impl Iterator<Item = &'a ClassifiedDelta> {
    counts.keys().flat_map(|category| {
        diff.deltas.iter().filter(move |delta| delta.meaning.category() == *category)
    })
}

/// How two blocks of free text differ, without quoting either.
///
/// Stated as "N differ, M added/removed" rather than as one number: a
/// positional comparison of texts of different lengths counts every
/// line after an inserted one as changed, and WinCUPL headers routinely
/// differ by a present-or-absent line. One number there would be
/// confidently wrong in the common case.
fn line_summary(before: &str, after: &str) -> String {
    let (a, b) = (before.lines().count(), after.lines().count());
    let differing = before.lines().zip(after.lines()).filter(|(x, y)| x != y).count();
    let length = a.abs_diff(b);
    if length == 0 {
        format!("{differing} of {a} line(s) differ (text not shown: banner, timestamp, serial)")
    } else {
        format!(
            "{a} line(s) -> {b}, {differing} differing in the common prefix \
             (text not shown: banner, timestamp, serial)"
        )
    }
}

/// The identifiers of a set of unmodelled fields, without their bodies.
fn identifiers(fields: &[String]) -> String {
    if fields.is_empty() {
        return "none".to_owned();
    }
    let mut names: Vec<&str> =
        fields.iter().map(|field| field.split_whitespace().next().unwrap_or("?")).collect();
    names.sort_unstable();
    names.join(" ")
}

/// The security bit in words. `None` is the file saying nothing, which
/// is not the same as saying the part is readable.
fn describe_security(state: Option<bool>) -> &'static str {
    match state {
        None => "not stated",
        Some(false) => "clear",
        Some(true) => "SET",
    }
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
        encode_with(Footprint::Gal, build)
    }

    fn encode_with(
        footprint: Footprint,
        build: impl FnOnce(&mut decpld_device::PhysicalDesign),
    ) -> FuseMap {
        let mut design = blank_design().expect("a blank design");
        build(&mut design);
        encode_design(&design, footprint).expect("encodable")
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
        assert_eq!(delta.counts_by_category(), [("matrix-output-enable", 1)].into_iter().collect());
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
        assert_eq!(
            delta.counts_by_category(),
            [("matrix-output-enable", 44)].into_iter().collect()
        );
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
            delta.counts_by_category(),
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
    fn no_free_text_field_is_ever_echoed() {
        // WinCUPL's banner carries a timestamp, the design's name, and
        // the installation's serial number, and CLAUDE.md forbids
        // redistributing that. It is not only the header: a `.pld`
        // author can put anything in an `N` note, and an unmodelled
        // field is reproduced identifier AND body. `oracle diff` output
        // is what gets pasted into an evidence file, so the rule has to
        // hold for every text field — this plants the same serial in
        // all three and requires none of them back.
        const SERIAL: &str = "MW-10400000";
        let mut before = in2();
        let mut after = in2();
        before.design_specification = format!("CUPL(WM) 5.0a Serial# {SERIAL}\nName in2\n");
        after.design_specification = format!("CUPL(WM) 5.0a Serial# {SERIAL}\nName oe-var\n");
        after.notes = vec![format!("built by {SERIAL}")];
        after.unknown_fields = vec![decpld_jedec::JedecField {
            identifier: "QP".to_owned(),
            body: format!("24 {SERIAL}"),
            span: decpld_diagnostics::Span::new(
                decpld_diagnostics::FileId(0),
                decpld_diagnostics::TextRange::new(0, 0),
            ),
        }];

        let delta = diff(&before, &after, Device::Atf22v10c).expect("this device");
        let text = render(&delta, "a.jed", "b.jed");
        assert!(!text.contains(SERIAL), "a serial reached stdout:\n{text}");
        assert!(!text.contains("Name in2"), "nor may the header text:\n{text}");
        assert!(!text.contains("built by"), "nor a note's text:\n{text}");
        // The identifier of an unmodelled field is reported; its body
        // is what can carry anything.
        assert!(text.contains("unmodelled fields: none -> QP"), "{text}");
        assert!(text.contains("notes: 0 before, 1 after"), "{text}");
    }

    #[test]
    fn a_header_that_gained_a_line_is_not_reported_as_every_line_changed() {
        // A positional comparison counts every line after an inserted
        // one as changed, and WinCUPL headers routinely differ by a
        // present-or-absent line — so one number would be confidently
        // wrong in the common case rather than in a corner.
        let mut before = in2();
        let mut after = in2();
        before.design_specification = "one\ntwo\nthree\n".to_owned();
        after.design_specification = "zero\none\ntwo\nthree\n".to_owned();

        let text = render(&diff(&before, &after, Device::Atf22v10c).expect("ok"), "a", "b");
        assert!(text.contains("3 line(s) -> 4"), "{text}");
        assert!(!text.contains("4 of"), "must not claim four lines changed:\n{text}");
    }

    #[test]
    fn a_security_fuse_change_is_reported_in_words() {
        // CLAUDE.md → Safety: the readback lock is the one irreversible
        // change. "None -> Some(true)" is Rust's `Debug`; "not stated
        // -> SET" is the sentence, and silence is not the same as
        // "clear".
        let before = in2();
        let mut after = in2();
        after.security = Some(true);
        let text = render(&diff(&before, &after, Device::Atf22v10c).expect("ok"), "a", "b");
        assert!(text.contains("security fuse: not stated -> SET"), "{text}");
    }

    #[test]
    fn a_pal_mode_file_is_refused_against_a_gal_mode_one() {
        // Both are this device, so a check that only asked "is this a
        // count this part has" passes — and then not one fuse is
        // comparable, so the report says "no fuse changed". A wrong
        // answer, where a refusal was available.
        let pal = file_of(&encode_with(Footprint::Pal, |_| {}));
        let error = diff(&in2(), &pal, Device::Atf22v10c).expect_err("different footprints");
        assert!(
            matches!(error, InspectError::FootprintMismatch { .. }),
            "expected a footprint mismatch, got {error:?}"
        );
        // And in the other order.
        assert!(diff(&pal, &in2(), Device::Atf22v10c).is_err());
    }

    #[test]
    fn the_rendered_report_is_what_a_reader_gets() {
        // One snapshot of a real report. The unit tests above assert
        // the *structs*; nothing asserted a rendered delta line, so a
        // `1 -> 0` printed as `0 -> 1` passed the whole suite — and on
        // this device family that inverts the connected/disconnected
        // reading of an array cell, which is how a wrong convention
        // would enter `targets/evidence/`.
        let block =
            G.row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin")).expect("a block");
        let changed = file_of(&encode(|design| {
            let cell = cell(design, 23);
            cell.mode = MacrocellMode::Registered;
            cell.oe_term = Some(PlacedCube {
                row: ProductTermId(block.output_enable_row),
                cube: Cube::new([literal(3, Polarity::True)]),
            });
            cell.data_terms = vec![PlacedCube {
                row: ProductTermId(block.data_rows.start),
                cube: Cube::new([literal(4, Polarity::True)]),
            }];
        }));
        let delta = diff(&in2(), &changed, Device::Atf22v10c).expect("this device");
        insta::assert_snapshot!(render(&delta, "before.jed", "after.jed"));
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
