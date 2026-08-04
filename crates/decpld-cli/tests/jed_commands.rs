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
    let strict = decpld(&["jed", "validate", &arg(&file), "--mode", "strict"]);
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
    let check = decpld(&["jed", "validate", &arg(&output), "--mode", "strict"]);
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
    let file = dir.write("warn.jed", "\x02h*QF8*F0*L0 11110000*Zzz*\x030000");

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
    let file = dir.write("warn.jed", "\x02h*QF8*F0*L0 11110000*Zzz*\x030000");

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
    let good = dir.write("a.jed", "\x02h*QF8*F0*L0 11110000*Zzz*\x030000");
    let bad = dir.write("b.jed", "not a jedec file at all");

    let out = decpld(&["jed", "diff", &arg(&good), &arg(&bad)]);
    assert_eq!(out.status.code(), Some(2), "an unreadable input is trouble, not a finding");
    let errors = stderr(&out);
    assert!(errors.contains("a.jed"), "the first file's diagnostics are missing: {errors:?}");
    assert!(errors.contains("b.jed"), "the failure itself must be reported: {errors:?}");
}
