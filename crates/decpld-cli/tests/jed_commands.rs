//! End-to-end tests of the `decpld jed` commands.
//!
//! These run the real binary via `CARGO_BIN_EXE_decpld`, so they cover
//! the wiring that unit tests cannot: argument parsing, file I/O, what
//! lands on stdout versus stderr, and the exit code. Everything with a
//! decision in it is tested as a pure function elsewhere; what is left
//! here is precisely the part that only a process can exercise.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const GALETTE: &str =
    include_str!("../../../targets/fixtures/jedec/galette-gal16v8-combinatorial.jed");

/// A scratch directory unique to one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("decpld-cli-{name}"));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, contents).expect("write fixture");
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn decpld(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_decpld")).args(args).output().expect("run decpld")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn arg(path: &Path) -> String {
    path.display().to_string()
}

// ---- validate ----

#[test]
fn validate_accepts_a_real_file_and_exits_zero() {
    let dir = TempDir::new("validate-ok");
    let file = dir.write("good.jed", GALETTE);

    let out = decpld(&["jed", "validate", &arg(&file)]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("2194 fuses"), "{text}");
    assert!(text.contains("403E"), "should report the checksum: {text}");
}

#[test]
fn validate_rejects_a_broken_file_and_exits_nonzero() {
    let dir = TempDir::new("validate-bad");
    let file = dir.write("bad.jed", "\x02h*QF8*F0*L0 1012*\x030000");

    let out = decpld(&["jed", "validate", &arg(&file)]);
    assert!(!out.status.success(), "should fail");
    let errors = stderr(&out);
    assert!(errors.contains("E3012"), "should name the diagnostic code: {errors}");
    // Errors go to stderr so `decpld jed validate f | ...` stays clean.
    assert!(!stdout(&out).contains("E3012"));
}

#[test]
fn validate_reports_the_line_and_column_of_a_fault() {
    let dir = TempDir::new("validate-span");
    let file = dir.write("bad.jed", "\x02h*\nQF8*\nF0*\nL0 1012*\n\x030000");

    let out = decpld(&["jed", "validate", &arg(&file)]);
    let errors = stderr(&out);
    assert!(errors.contains("bad.jed:4:"), "should locate the fault: {errors}");
    assert!(errors.contains('^'), "should point at it: {errors}");
}

#[test]
fn strict_mode_rejects_what_the_default_mode_accepts() {
    // A file with no transmission checksum: fine on disk, not conformant.
    let dir = TempDir::new("validate-strict");
    let file = dir.write("nosum.jed", "\x02h*QF8*F0*L0 11110000*\x03");

    assert!(decpld(&["jed", "validate", &arg(&file)]).status.success());
    let strict = decpld(&["jed", "validate", &arg(&file), "--strictness", "strict"]);
    assert!(!strict.status.success(), "strict should reject: {}", stdout(&strict));
}

#[test]
fn a_missing_file_is_an_error_not_a_panic() {
    let out = decpld(&["jed", "validate", "/nonexistent/decpld/nope.jed"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("decpld:"), "{}", stderr(&out));
}

#[test]
fn exit_codes_distinguish_a_finding_from_trouble() {
    // `diff(1)` convention: 0 nothing to report, 1 a finding, 2 trouble.
    // Collapsing them would force every caller to parse stderr to tell
    // "these files differ" from "I could not read that file".
    let dir = TempDir::new("exit-codes");
    let good = dir.write("good.jed", GALETTE);
    let invalid = dir.write("invalid.jed", "\x02h*QF8*F0*L0 1012*\x030000");

    assert_eq!(decpld(&["jed", "validate", &arg(&good)]).status.code(), Some(0));
    assert_eq!(
        decpld(&["jed", "validate", &arg(&invalid)]).status.code(),
        Some(1),
        "an invalid file is a finding: the command answered the question"
    );
    assert_eq!(
        decpld(&["jed", "validate", "/nonexistent/decpld/nope.jed"]).status.code(),
        Some(2),
        "an unreadable file is trouble: the command could not answer at all"
    );
}

#[test]
fn diff_exits_one_for_a_difference_and_two_for_an_unreadable_file() {
    let dir = TempDir::new("diff-exit");
    let a = dir.write("a.jed", "\x02h*QF16*F0*L0 1010000000000000*\x030000");
    let b = dir.write("b.jed", "\x02h*QF16*F0*L0 1011000000000000*\x030000");

    assert_eq!(decpld(&["jed", "diff", &arg(&a), &arg(&a)]).status.code(), Some(0));
    assert_eq!(decpld(&["jed", "diff", &arg(&a), &arg(&b)]).status.code(), Some(1));
    assert_eq!(
        decpld(&["jed", "diff", &arg(&a), "/nonexistent/decpld/nope.jed"]).status.code(),
        Some(2)
    );
}

#[test]
fn diff_notices_a_dropped_test_vector() {
    // The fuses are identical; a test vector vanished. Reporting "same
    // device" would bless exactly the silent data loss that
    // preserve-unknown mode exists to prevent.
    let dir = TempDir::new("diff-vectors");
    let a = dir.write("a.jed", "\x02h*QF8*F0*L0 11110000*V0001 XXXX*\x030000");
    let b = dir.write("b.jed", "\x02h*QF8*F0*L0 11110000*\x030000");

    let out = decpld(&["jed", "diff", &arg(&a), &arg(&b)]);
    assert_eq!(out.status.code(), Some(1), "stdout: {}", stdout(&out));
    assert!(stdout(&out).contains("unmodelled fields"), "{}", stdout(&out));
}

#[test]
fn canonicalize_puts_value_fields_before_the_fuse_list() {
    // JEDEC 3A requires value fields (QF/QP/QV) before any programming
    // or testing field. deCPLD's own parser does two passes and would
    // not care; other tools may.
    let dir = TempDir::new("canon-order");
    let input = dir.write("in.jed", "\x02h*QF16*QP20*F0*L0 1010000000000000*\x030000");

    let out = decpld(&["jed", "canonicalize", &arg(&input)]);
    assert!(out.status.success());
    let text = stdout(&out);
    let qp = text.find("QP20*").expect("QP retained");
    let l = text.find("L0").expect("an L field");
    assert!(qp < l, "value fields must come first:\n{text}");
}

// ---- canonicalize ----

#[test]
fn canonicalize_writes_a_file_that_validates() {
    let dir = TempDir::new("canon");
    let input = dir.write("in.jed", GALETTE);
    let output = dir.path("out.jed");

    let out = decpld(&["jed", "canonicalize", &arg(&input), "-o", &arg(&output)]);
    assert!(out.status.success(), "stderr: {}", stderr(&out));

    // Strict mode is the strongest available check that the rewritten
    // file carries correct checksums: it verifies both.
    let check = decpld(&["jed", "validate", &arg(&output), "--strictness", "strict"]);
    assert!(check.status.success(), "canonical output must be strictly valid: {}", stderr(&check));
}

#[test]
fn canonicalize_preserves_the_device_it_was_given() {
    // The check that matters: rewriting must not change the fuses. `jed
    // diff` reporting "same device" is exactly that assertion, made by
    // the tool itself.
    let dir = TempDir::new("canon-same");
    let input = dir.write("in.jed", GALETTE);
    let output = dir.path("out.jed");

    decpld(&["jed", "canonicalize", &arg(&input), "-o", &arg(&output)]);
    let diff = decpld(&["jed", "diff", &arg(&input), &arg(&output)]);
    assert!(diff.status.success(), "canonicalize changed the device: {}", stdout(&diff));
    assert!(stdout(&diff).contains("same device"));
}

#[test]
fn canonicalize_repairs_a_missing_fuse_checksum() {
    // C0000 means "not computed". Canonicalising should produce a real
    // checksum rather than propagating the input's silence.
    let dir = TempDir::new("canon-checksum");
    let input = dir.write("in.jed", "\x02h*QF16*F0*L0 1011000000000000*C0000*\x030000");

    let out = decpld(&["jed", "canonicalize", &arg(&input)]);
    assert!(out.status.success(), "{}", stderr(&out));
    let text = stdout(&out);
    assert!(!text.contains("C0000*"), "checksum should be recomputed: {text}");
    assert!(text.contains("C000D*"), "expected C000D: {text}");
}

#[test]
fn canonicalize_defaults_to_stdout() {
    let dir = TempDir::new("canon-stdout");
    let input = dir.write("in.jed", "\x02h*QF8*F0*L0 11110000*\x030000");

    let out = decpld(&["jed", "canonicalize", &arg(&input)]);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with('\u{2}'), "output should be a JEDEC file");
}

#[test]
fn the_compact_style_is_smaller_than_the_canonical_one() {
    let dir = TempDir::new("canon-style");
    let input = dir.write("in.jed", GALETTE);

    let canonical = decpld(&["jed", "canonicalize", &arg(&input)]);
    let compact = decpld(&["jed", "canonicalize", &arg(&input), "--style", "compact"]);
    assert!(compact.stdout.len() < canonical.stdout.len());
}

// ---- diff ----

#[test]
fn diff_reports_no_change_between_a_file_and_itself() {
    let dir = TempDir::new("diff-same");
    let file = dir.write("a.jed", GALETTE);

    let out = decpld(&["jed", "diff", &arg(&file), &arg(&file)]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("same device"));
}

#[test]
fn diff_names_the_fuse_that_changed() {
    let dir = TempDir::new("diff-one");
    let a = dir.write("a.jed", "\x02h*QF16*F0*L0 1010000000000000*\x030000");
    let b = dir.write("b.jed", "\x02h*QF16*F0*L0 1011000000000000*\x030000");

    let out = decpld(&["jed", "diff", &arg(&a), &arg(&b)]);
    assert!(!out.status.success(), "a difference is a nonzero exit, like diff(1)");
    let text = stdout(&out);
    assert!(text.contains("fuse 3: 0 -> 1"), "{text}");
    assert!(text.contains("1 fuse(s) differ"), "{text}");
}

#[test]
fn diff_ignores_formatting_and_compares_the_device() {
    // The reason `jed diff` exists rather than `diff a.jed b.jed`: these
    // two files share almost no bytes and are the same device.
    let dir = TempDir::new("diff-format");
    let a = dir.write("a.jed", "\x02h*QF16*F0*L0 1010000000000000*\x030000");
    let b =
        dir.write("b.jed", "\x02h*\r\nQF16*\r\nF0*\r\nL0 10100000*\r\nL8 00000000*\r\n\x030000");

    let out = decpld(&["jed", "diff", &arg(&a), &arg(&b)]);
    assert!(out.status.success(), "{}", stdout(&out));
    assert!(stdout(&out).contains("same device"));
}

// ---- general ----

#[test]
fn help_lists_the_jed_commands() {
    let out = decpld(&["jed", "--help"]);
    assert!(out.status.success());
    let text = stdout(&out);
    for command in ["validate", "canonicalize", "diff"] {
        assert!(text.contains(command), "`{command}` missing from help:\n{text}");
    }
}

#[test]
fn an_unknown_command_fails_without_panicking() {
    let out = decpld(&["jed", "frobnicate"]);
    assert!(!out.status.success());
    assert!(!stderr(&out).contains("panicked"), "{}", stderr(&out));
}

// ---- Stream routing (second review round) ----
//
// The point of the change these cover: which stream carries a
// diagnostic must depend on it being a diagnostic, never on whether the
// parse happened to succeed. A warning on stdout corrupts every
// `decpld jed … > file` and every pipeline.

#[test]
fn a_successful_parse_still_sends_its_warnings_to_stderr() {
    // A file that parses *and* warns is the case that was missed: the
    // error path already went to stderr, so a test that only used a
    // broken file would have passed against the old behaviour too.
    let dir = TempDir::new("validate-warns");
    // `123` has no field identifier at all. `Z` would NOT do: JEDEC 3A
    // reserves it and tells receivers to ignore such fields, so deCPLD
    // says nothing about them.
    let file = dir.write("warn.jed", "\x02h*QF8*F0*L0 11110000*123*\x030000");

    let out = decpld(&["jed", "validate", &arg(&file)]);
    assert_eq!(out.status.code(), Some(0), "a file with only warnings is valid");
    assert!(stdout(&out).contains("ok — 8 fuses"), "summary on stdout: {:?}", stdout(&out));
    // Assert the warning is genuinely produced *and* lands on stderr.
    // Only checking that stdout lacks it would pass against a file that
    // never warned at all, which is how a routing test quietly stops
    // testing routing.
    assert!(stderr(&out).contains("warning"), "warning must reach stderr: {:?}", stderr(&out));
    assert!(!stdout(&out).contains("warning"), "and never stdout: {:?}", stdout(&out));
}

#[test]
fn canonicalize_keeps_stdout_a_pure_jedec_file_when_the_input_warns() {
    // This is the whole reason diagnostics were moved to stderr:
    // `decpld jed canonicalize in.jed > out.jed` must produce a file a
    // programmer can read, not one with a warning glued to the front.
    let dir = TempDir::new("canon-warns");
    let file = dir.write("warn.jed", "\x02h*QF8*F0*L0 11110000*123*\x030000");

    let out = decpld(&["jed", "canonicalize", &arg(&file)]);
    assert_eq!(out.status.code(), Some(0));

    let text = stdout(&out);
    assert!(text.starts_with('\u{2}'), "stdout must begin with STX: {text:?}");
    assert!(!text.contains("warning"), "diagnostic leaked onto stdout: {text:?}");

    // And the diagnostics did not simply vanish.
    let written = dir.write("out.jed", &text);
    assert_eq!(
        decpld(&["jed", "validate", &arg(&written)]).status.code(),
        Some(0),
        "the captured stdout must itself be a valid file"
    );
}

#[test]
fn diff_reports_the_first_files_diagnostics_even_when_the_second_is_unreadable() {
    // The `?` on the second parse used to return before either file's
    // diagnostics were printed, so a warning about a.jed disappeared
    // whenever b.jed was broken — indistinguishable from a.jed being
    // clean.
    let dir = TempDir::new("diff-warn-then-fail");
    let good = dir.write("a.jed", "\x02h*QF8*F0*L0 11110000*123*\x030000");
    let bad = dir.write("b.jed", "not a jedec file at all");

    let out = decpld(&["jed", "diff", &arg(&good), &arg(&bad)]);
    assert_eq!(out.status.code(), Some(2), "an unreadable input is trouble, not a finding");
    let errors = stderr(&out);
    assert!(errors.contains("a.jed"), "the first file's diagnostics are missing: {errors:?}");
    assert!(errors.contains("b.jed"), "the failure itself must be reported: {errors:?}");
}

// ---------------------------------------------------------------------
// `jed inspect` — SPEC.md §6.3.
//
// The fixture is generated by the ATF22V10 encoder rather than checked
// in, so it cannot drift from the device model it is meant to exercise:
// a change that broke encoding would break this test rather than leave
// it passing against a stale file.
// ---------------------------------------------------------------------

/// A GAL-footprint file with pin 2 and pin 3 driving pin 23, registered
/// and active low.
fn atf22v10_file() -> String {
    use decpld_atf22v10::{
        Atf22v10Geometry, Footprint, blank_design, bool_input_of_source, encode_design,
    };
    use decpld_device::{
        MacrocellId, MacrocellMode, OutputPolarity, PinNumber, PlacedCube, ProductTermId,
    };
    use decpld_jedec::{FuseVector, JedecFile, WriterStyle, write};
    use decpld_logic::{Cube, Literal, Polarity};

    let geometry = Atf22v10Geometry;
    let mut design = blank_design().expect("a blank design");
    let macrocell = geometry.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let block = geometry.row_block(macrocell).expect("a measured block");
    let literal = |pin: u8, polarity| {
        let source = geometry.source_of_pin(PinNumber(pin)).expect("a signal pin");
        Literal::new(bool_input_of_source(source.index), polarity)
    };
    let cell = design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present");
    cell.mode = MacrocellMode::Registered;
    cell.polarity = OutputPolarity::ActiveLow;
    cell.oe_term =
        Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
    cell.data_terms = vec![PlacedCube {
        row: ProductTermId(block.data_rows.start),
        cube: Cube::new([literal(2, Polarity::True), literal(3, Polarity::Complement)]),
    }];

    let fuses = encode_design(&design, Footprint::Gal).expect("encodable");
    let mut vector = FuseVector::new(fuses.len(), false);
    for (index, state) in fuses.iter().enumerate() {
        let index = u32::try_from(index).expect("under six thousand");
        vector.set(index, state).expect("in range");
    }
    let file = JedecFile {
        design_specification: "inspect fixture\n".to_owned(),
        fuses: vector,
        default_fuse: Some(false),
        notes: Vec::new(),
        security: None,
        fuse_checksum: None,
        transmission_checksum: None,
        unknown_fields: Vec::new(),
    };
    write(&file, WriterStyle::Compact).expect("writable")
}

#[test]
fn inspect_reads_a_fuse_map_back_as_pins_and_equations() {
    let dir = TempDir::new("inspect-text");
    let file = dir.write("design.jed", &atf22v10_file());
    let output = decpld(&["jed", "inspect", &arg(&file), "--device", "ATF22V10C"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    assert!(text.contains("ATF22V10C  DIP24  5892 fuses"), "{text}");
    assert!(text.contains("macrocell 9 -> pin 23"), "{text}");
    assert!(text.contains("registered"), "{text}");
    assert!(text.contains("active low"), "{text}");
    assert!(text.contains("pin2 & !pin3"), "{text}");
    // The report goes to stdout so it can be piped; diagnostics do not.
    assert!(stderr(&output).is_empty(), "{}", stderr(&output));
}

#[test]
fn inspect_emits_json_that_parses() {
    let dir = TempDir::new("inspect-json");
    let file = dir.write("design.jed", &atf22v10_file());
    let output = decpld(&["jed", "inspect", &arg(&file), "--device", "ATF22V10C", "--json"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let text = stdout(&output);
    // Parsed rather than string-matched: a report that is not valid
    // JSON is broken for the machine consumers `--json` exists for, and
    // `contains` would not notice a missing brace.
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
    assert_eq!(value["device"], "ATF22V10C");
    assert_eq!(value["fuse_count"], 5892);
    assert_eq!(value["macrocells"][9]["pin"], 23);
    assert_eq!(value["macrocells"][9]["sum"][0]["text"], "pin2 & !pin3");
    assert_eq!(value["macrocells"][9]["sum"][0]["literals"][1]["complemented"], true);
}

#[test]
fn inspect_refuses_a_file_for_another_part_as_trouble_not_as_a_finding() {
    // The GAL16V8 fixture is 2194 fuses. Asked to read it as an
    // ATF22V10C the command cannot do its job at all, which is exit 2 —
    // not exit 1, which would mean it looked and found something.
    let dir = TempDir::new("inspect-wrong-part");
    let file = dir.write("gal16v8.jed", GALETTE);
    let output = decpld(&["jed", "inspect", &arg(&file), "--device", "ATF22V10C"]);

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("2194"), "{}", stderr(&output));
    assert!(stdout(&output).is_empty(), "no report for a file it could not read");
}

#[test]
fn inspect_checks_the_package_it_is_given_rather_than_applying_it() {
    let dir = TempDir::new("inspect-package");
    let file = dir.write("design.jed", &atf22v10_file());

    let agreed =
        decpld(&["jed", "inspect", &arg(&file), "--device", "ATF22V10C", "--package", "DIP24"]);
    assert!(agreed.status.success(), "{}", stderr(&agreed));

    let disagreed =
        decpld(&["jed", "inspect", &arg(&file), "--device", "ATF22V10C", "--package", "PLCC28"]);
    assert_eq!(disagreed.status.code(), Some(2));
    assert!(stderr(&disagreed).contains("checked, never applied"), "{}", stderr(&disagreed));
}

#[test]
fn inspect_refuses_a_file_whose_declared_checksum_is_a_lie() {
    // Exit 2, and no report. The parser gates this — a `C` field
    // disagreeing with the fuse data means one of the two is wrong, and
    // decoding the fuses anyway would describe a device that may not be
    // the one the file was meant to program. `jed inspect` therefore
    // has no finding path of its own: every file it can describe has
    // already had its checksum checked.
    let dir = TempDir::new("inspect-bad-checksum");
    let honest = atf22v10_file();
    let lying = honest.replacen("*\nC", "*\nC0001*\nN was C", 1);
    assert_ne!(lying, honest, "the fixture must carry a C field to corrupt");
    let file = dir.write("design.jed", &lying);

    let output = decpld(&["jed", "inspect", &arg(&file), "--device", "ATF22V10C"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("fuse checksum mismatch"), "{}", stderr(&output));
    assert!(stdout(&output).is_empty(), "no report for a file that failed its own checksum");
}

#[test]
fn inspect_names_the_device_it_needs_when_none_is_given() {
    let dir = TempDir::new("inspect-no-device");
    let file = dir.write("design.jed", &atf22v10_file());
    let output = decpld(&["jed", "inspect", &arg(&file)]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--device"), "{}", stderr(&output));
}
