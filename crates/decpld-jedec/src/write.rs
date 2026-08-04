//! Writing a [`JedecFile`] back out.

use crate::model::JedecFile;
use crate::transmission_checksum;

/// How the output is laid out. SPEC.md §5.6.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WriterStyle {
    /// Every fuse stated explicitly, one field per line, 32 fuses per
    /// `L` field. Verbose, diffable, and never ambiguous about what a
    /// fuse is set to — the default, and what `decpld jed canonicalize`
    /// produces.
    #[default]
    Canonical,

    /// Only fuses differing from the `F` default, grouped into maximal
    /// runs. Much smaller for sparse designs, and closer to what real
    /// assemblers emit.
    Compact,
    //
    // NOT IMPLEMENTED: `WinCuplComparable`. SPEC.md lists it, and it
    // will be needed for byte-level oracle comparisons — but matching
    // WinCUPL's layout requires having WinCUPL's output to match, and
    // this project has none yet. Writing it now would mean inventing a
    // format from memory and calling it "WinCUPL-comparable", which is
    // exactly the unevidenced guess CLAUDE.md forbids. It lands in M1
    // with the oracle harness, against real captured files.
}

/// Why a file could not be written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum WriteError {
    /// Free text contained `*`, which terminates a JEDEC field.
    ///
    /// The format has no escape mechanism, so this cannot be encoded.
    /// Refusing is the only honest option: emitting it would produce a
    /// file that silently reads back as something else.
    #[error("{context} contains an asterisk, which would terminate the field early: {text:?}")]
    AsteriskInText { context: &'static str, text: String },
}

/// Serialise `file` in the given style.
///
/// The result always carries a correct transmission checksum and a
/// recomputed fuse checksum, so writing a file is also how it gets
/// repaired.
pub fn write(file: &JedecFile, style: WriterStyle) -> Result<String, WriteError> {
    let mut out = String::from('\u{2}');

    // The design specification is the first field and has no identifier.
    let header = &file.design_specification;
    reject_asterisk("the design specification", header)?;
    out.push_str(header);
    out.push_str("*\n");

    out.push_str(&format!("QF{}*\n", file.fuses.len()));
    out.push_str(if file.default_fuse { "F1*\n" } else { "F0*\n" });

    for note in &file.notes {
        reject_asterisk("a note", note)?;
        out.push_str(&format!("N {note}*\n"));
    }

    // Silence and an explicit instruction are different facts, so a
    // security bit that was never mentioned is not invented here.
    if let Some(secure) = file.security {
        out.push_str(if secure { "G1*\n" } else { "G0*\n" });
    }

    match style {
        WriterStyle::Canonical => write_all_fuses(&mut out, file),
        WriterStyle::Compact => write_differing_fuses(&mut out, file),
    }

    // Recomputed, never copied: a file declaring `C0000` ("not
    // computed") should come out of canonicalisation carrying a real
    // checksum, not inherit the input's failure to have one.
    out.push_str(&format!("C{:04X}*\n", file.fuses.checksum()));

    for field in &file.unknown_fields {
        reject_asterisk("an unmodelled field", &field.body)?;
        out.push_str(&format!("{}{}*\n", field.identifier, field.body));
    }

    out.push('\u{3}');
    // The checksum covers STX through ETX inclusive, so it can only be
    // computed once everything before it exists.
    let checksum = transmission_checksum(out.as_bytes());
    out.push_str(&format!("{checksum:04X}\n"));

    Ok(out)
}

/// Every fuse, in fields of 32, on their own lines.
fn write_all_fuses(out: &mut String, file: &JedecFile) {
    const PER_FIELD: u32 = 32;
    let states: Vec<bool> = file.fuses.iter().collect();

    for start in (0..file.fuses.len()).step_by(PER_FIELD as usize) {
        let end = (start + PER_FIELD).min(file.fuses.len());
        // Zero-padded to four digits so fuse numbers align in a diff.
        out.push_str(&format!("L{start:04} "));
        for state in &states[start as usize..end as usize] {
            out.push(if *state { '1' } else { '0' });
        }
        out.push_str("*\n");
    }
}

/// Only runs that differ from the default state.
fn write_differing_fuses(out: &mut String, file: &JedecFile) {
    let states: Vec<bool> = file.fuses.iter().collect();
    let default = file.default_fuse;

    let mut index = 0usize;
    while index < states.len() {
        if states[index] == default {
            index += 1;
            continue;
        }
        let start = index;
        while index < states.len() && states[index] != default {
            index += 1;
        }
        out.push_str(&format!("L{start} "));
        for state in &states[start..index] {
            out.push(if *state { '1' } else { '0' });
        }
        out.push_str("*\n");
    }
}

fn reject_asterisk(context: &'static str, text: &str) -> Result<(), WriteError> {
    if text.contains('*') {
        return Err(WriteError::AsteriskInText { context, text: text.to_owned() });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParserMode, parse, parse_with_mode};
    use decpld_diagnostics::FileId;

    const FILE: FileId = FileId(0);

    fn round_trip(text: &str, style: WriterStyle) -> crate::JedecFile {
        let original = parse(text, FILE).expect("fixture parses").file;
        let written = write(&original, style).expect("writes");
        let reparsed = parse(&written, FILE)
            .unwrap_or_else(|bundle| {
                let messages: Vec<_> = bundle.iter().map(|d| d.headline()).collect();
                panic!("our own output did not parse: {messages:#?}\n---\n{written}");
            })
            .file;
        assert!(
            original.describes_same_device_as(&reparsed),
            "round trip changed the file\n---\n{written}\n---\nbefore: {original:#?}\nafter: {reparsed:#?}"
        );
        // Both checksums are regenerated rather than copied — see
        // `describes_same_device_as` — so assert they are *correct*
        // rather than *unchanged*. This is the check that would catch a
        // writer emitting a stale or absent checksum.
        assert_eq!(
            reparsed.fuse_checksum,
            Some(reparsed.computed_fuse_checksum()),
            "written file must carry a correct fuse checksum"
        );
        assert!(reparsed.transmission_checksum.is_some());
        reparsed
    }

    const SAMPLE: &str =
        "\x02a design*QF32*F0*L0 10110000111100001010101011110000*C0000*N a note*G0*\x030000";

    // ---- Round trip ----

    #[test]
    fn canonical_output_round_trips() {
        round_trip(SAMPLE, WriterStyle::Canonical);
    }

    #[test]
    fn compact_output_round_trips() {
        round_trip(SAMPLE, WriterStyle::Compact);
    }

    #[test]
    fn a_real_galette_file_round_trips() {
        // The strongest round-trip case available: a file this project
        // did not author, with 2194 fuses and a header that must survive
        // verbatim.
        let text =
            include_str!("../../../targets/fixtures/jedec/galette-gal16v8-combinatorial.jed");
        round_trip(text, WriterStyle::Canonical);
        round_trip(text, WriterStyle::Compact);
    }

    #[test]
    fn unmodelled_fields_survive_a_round_trip() {
        // The whole point of preserve-unknown: a rewrite must not delete
        // the test vectors of a file it did not fully understand.
        let text = "\x02h*QF8*F0*QP20*V0001 XXXX*L0 11110000*\x030000";
        let reparsed = round_trip(text, WriterStyle::Canonical);
        let kept: Vec<&str> =
            reparsed.unknown_fields.iter().map(|f| f.identifier.as_str()).collect();
        assert_eq!(kept, ["QP", "V"]);
    }

    // ---- Checksums ----

    #[test]
    fn the_written_transmission_checksum_verifies() {
        // The checksum covers the bytes it is appended to, so it can
        // only be computed after everything else is emitted. Getting
        // this wrong produces a file that fails its own check — which is
        // why re-parsing in strict mode is the assertion.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let written = write(&file, WriterStyle::Canonical).unwrap();
        parse_with_mode(&written, FILE, ParserMode::Strict)
            .expect("our output must satisfy its own transmission checksum");
    }

    #[test]
    fn the_written_fuse_checksum_is_recomputed_not_copied() {
        // The input declares C0000 ("not computed"). A writer that
        // copied the declared value would propagate a file's failure to
        // carry a real checksum; canonicalising should fix it.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let expected = file.computed_fuse_checksum();
        let written = write(&file, WriterStyle::Canonical).unwrap();
        assert!(
            written.contains(&format!("C{expected:04X}")),
            "expected C{expected:04X} in:\n{written}"
        );
    }

    // ---- Style ----

    #[test]
    fn canonical_style_is_one_field_per_line() {
        let file = parse(SAMPLE, FILE).unwrap().file;
        let written = write(&file, WriterStyle::Canonical).unwrap();
        assert!(written.contains("QF32*\n"), "got:\n{written}");
        assert!(written.contains("F0*\n"));
    }

    #[test]
    fn compact_style_emits_only_fuses_that_differ_from_the_default() {
        // 32 fuses, all default except one. Canonical states every fuse;
        // compact says only what is surprising.
        let text = "\x02h*QF32*F0*L7 1*\x030000";
        let file = parse(text, FILE).unwrap().file;

        let canonical = write(&file, WriterStyle::Canonical).unwrap();
        let compact = write(&file, WriterStyle::Compact).unwrap();

        assert!(compact.len() < canonical.len(), "compact should be smaller");
        assert!(canonical.contains("L0000 "), "canonical states every fuse from 0");
        assert!(compact.contains("L7 1*"), "compact names just the run: {compact}");
    }

    #[test]
    fn compact_output_of_an_all_default_device_has_no_fuse_list() {
        let text = "\x02h*QF32*F0*\x030000";
        let file = parse(text, FILE).unwrap().file;
        let compact = write(&file, WriterStyle::Compact).unwrap();
        assert!(!compact.contains("*L"), "nothing differs from the default: {compact}");
        // It must still round trip: the F field carries the whole story.
        round_trip(text, WriterStyle::Compact);
    }

    #[test]
    fn output_is_deterministic() {
        // SPEC.md §5.32: the same inputs must produce the same bytes.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let a = write(&file, WriterStyle::Canonical).unwrap();
        let b = write(&file, WriterStyle::Canonical).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn writing_is_idempotent_through_a_reparse() {
        // write -> parse -> write must reach a fixed point immediately,
        // or `decpld jed canonicalize` would churn files that are
        // already canonical.
        let file = parse(SAMPLE, FILE).unwrap().file;
        let once = write(&file, WriterStyle::Canonical).unwrap();
        let twice = write(&parse(&once, FILE).unwrap().file, WriterStyle::Canonical).unwrap();
        assert_eq!(once, twice);
    }

    // ---- Refusals ----

    #[test]
    fn a_note_containing_an_asterisk_is_refused() {
        // JEDEC has no escape mechanism: an asterisk inside a note would
        // terminate the field early and silently corrupt the file.
        // Refusing beats emitting something that reads back differently.
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.notes.push("this * ends the field".to_owned());
        assert!(matches!(
            write(&file, WriterStyle::Canonical),
            Err(WriteError::AsteriskInText { .. })
        ));
    }

    #[test]
    fn a_header_containing_an_asterisk_is_refused() {
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.design_specification = "bad * header".to_owned();
        assert!(matches!(
            write(&file, WriterStyle::Canonical),
            Err(WriteError::AsteriskInText { .. })
        ));
    }

    #[test]
    fn an_empty_header_round_trips_as_an_empty_header() {
        // JEDEC cannot express "no header" — the header IS the first
        // field — so an empty one is the floor, and it must survive.
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.design_specification = String::new();
        let written = write(&file, WriterStyle::Canonical).unwrap();
        let reparsed = parse(&written, FILE).unwrap().file;
        assert_eq!(reparsed.design_specification, "");
    }

    #[test]
    fn the_security_bit_is_only_written_when_it_was_present() {
        let mut file = parse(SAMPLE, FILE).unwrap().file;
        file.security = None;
        let written = write(&file, WriterStyle::Canonical).unwrap();
        assert!(!written.contains("*G"), "silence must stay silence: {written}");

        file.security = Some(true);
        assert!(write(&file, WriterStyle::Canonical).unwrap().contains("G1*"));
    }
}
