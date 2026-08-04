//! The `decpld` command-line driver.
//!
//! Only the `jed` family exists so far (M0). The rest of the surface —
//! `build`, `check`, `fmt`, `sim`, `report`, `oracle`, `program` — is
//! specified in SPEC.md Part VIII and arrives with the milestones that
//! give those commands something to do.
//!
//! Commands here stay thin: they read files, call the library, and print
//! what comes back. Anything with a decision in it belongs in a crate
//! where it can be tested without spawning a process.

mod render;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use decpld_diagnostics::FileId;
use decpld_jedec::{JedecDiff, ParserMode, WriterStyle, parse_with_mode, write};

#[derive(Parser)]
#[command(
    name = "decpld",
    version,
    about = "A compiler for ATF22V10 and ATF16V8 programmable logic",
    long_about = "deCPLD (\"decoupled\") compiles a modern hardware-description language \
                  to JEDEC fuse maps for Microchip ATF22V10 and ATF16V8 devices.\n\n\
                  The compiler is being built milestone by milestone; see PLAN.md. \
                  Today only the `jed` commands are available."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inspect and manipulate JEDEC files
    #[command(subcommand)]
    Jed(JedCommand),
}

#[derive(Subcommand)]
enum JedCommand {
    /// Check that a JEDEC file is well formed and its checksums agree
    Validate {
        file: PathBuf,
        /// How tolerant to be of what the file contains
        ///
        /// Named `--strictness` rather than `--mode` because `--mode`
        /// already means an ATF16V8 global mode (SPEC.md §5.16.3), which
        /// is the datasheet's own word for registered/complex/simple.
        /// `jed inspect --device` will report one, so the two would
        /// otherwise collide on a single command.
        #[arg(long, value_enum, default_value_t = Strictness::PreserveUnknown)]
        strictness: Strictness,
    },

    /// Rewrite a JEDEC file in a canonical form, repairing its checksums
    Canonicalize {
        file: PathBuf,
        /// Where to write; defaults to standard output
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Layout of the rewritten file
        #[arg(long, value_enum, default_value_t = Style::Canonical)]
        style: Style,
    },

    /// Compare two JEDEC files by fuse vector rather than by text
    Diff { before: PathBuf, after: PathBuf },
}

#[derive(Clone, Copy, ValueEnum)]
enum Strictness {
    /// Everything must conform to JEDEC 3A
    Strict,
    /// Accept what real tools emit; discard fields deCPLD does not model
    Compatible,
    /// Accept what real tools emit; keep unmodelled fields for rewriting
    PreserveUnknown,
}

#[derive(Clone, Copy, ValueEnum)]
enum Style {
    /// Every fuse stated, 32 per line
    Canonical,
    /// Only fuses differing from the default state
    Compact,
}

impl From<Strictness> for ParserMode {
    fn from(strictness: Strictness) -> Self {
        match strictness {
            Strictness::Strict => Self::Strict,
            Strictness::Compatible => Self::Compatible,
            Strictness::PreserveUnknown => Self::PreserveUnknown,
        }
    }
}

impl From<Style> for WriterStyle {
    fn from(style: Style) -> Self {
        match style {
            Style::Canonical => Self::Canonical,
            Style::Compact => Self::Compact,
        }
    }
}

/// Exit codes, following `diff(1)`: 0 nothing to report, 1 a finding, 2
/// trouble.
///
/// The distinction is what makes these commands scriptable. Collapsing
/// "the files differ" into the same code as "the file could not be read"
/// would force every caller to parse stderr to tell a result from a
/// failure. Clap already exits 2 on a usage error, which is trouble of
/// exactly the same kind.
const OK: u8 = 0;
const FINDINGS: u8 = 1;
const TROUBLE: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Jed(JedCommand::Validate { file, strictness }) => {
            validate(&file, strictness.into())
        }
        Command::Jed(JedCommand::Canonicalize { file, output, style }) => {
            canonicalize(&file, output.as_deref(), style.into())
        }
        Command::Jed(JedCommand::Diff { before, after }) => diff(&before, &after),
    };

    match result {
        Ok(code) => ExitCode::from(code),
        Err(Failure { message, code }) => {
            eprintln!("decpld: {message}");
            ExitCode::from(code)
        }
    }
}

/// A command failure: what went wrong, and how bad it is.
struct Failure {
    message: String,
    code: u8,
}

impl Failure {
    /// The input was unreadable or malformed — the command could not do
    /// its job at all.
    fn trouble(message: impl Into<String>) -> Self {
        Self { message: message.into(), code: TROUBLE }
    }
}

fn read(path: &Path) -> Result<String, Failure> {
    std::fs::read_to_string(path).map_err(|e| Failure::trouble(format!("{}: {e}", path.display())))
}

fn validate(path: &Path, mode: ParserMode) -> Result<u8, Failure> {
    let source = read(path)?;
    let name = path.display().to_string();

    match parse_with_mode(&source, FileId(0), mode) {
        Ok(parsed) => {
            // Diagnostics go to stderr whether or not the parse
            // succeeded. Which stream carries a diagnostic must depend on
            // it being a diagnostic, not on the luck of the parse — a
            // warning on stdout corrupts `decpld jed validate f | ...`.
            eprint!("{}", render::bundle(&parsed.diagnostics, &name, &source));
            let file = parsed.file;
            println!("{}", summarise(&name, &file));
            Ok(OK)
        }
        Err(bundle) => {
            eprint!("{}", render::bundle(&bundle, &name, &source));
            // A file failing validation is a *finding*: the command did
            // its job and the answer is "no".
            Err(Failure { message: format!("{name}: not a valid JEDEC file"), code: FINDINGS })
        }
    }
}

fn canonicalize(path: &Path, output: Option<&Path>, style: WriterStyle) -> Result<u8, Failure> {
    let source = read(path)?;
    let name = path.display().to_string();

    let parsed =
        parse_with_mode(&source, FileId(0), ParserMode::PreserveUnknown).map_err(|bundle| {
            eprint!("{}", render::bundle(&bundle, &name, &source));
            Failure::trouble(format!("{name}: not a valid JEDEC file"))
        })?;

    // Every command reports what the parse found. Three commands over
    // one file disagreeing about its diagnostics is the same class of
    // defect as `decpld check` disagreeing with an editor squiggle
    // (CLAUDE.md → the one architectural rule) — and `canonicalize` is
    // exactly where "I did not recognise this field but rewrote it
    // anyway" needs saying.
    eprint!("{}", render::bundle(&parsed.diagnostics, &name, &source));

    let text = write(&parsed.file, style).map_err(|e| Failure::trouble(format!("{name}: {e}")))?;

    match output {
        Some(destination) => std::fs::write(destination, &text)
            .map_err(|e| Failure::trouble(format!("{}: {e}", destination.display())))?,
        None => print!("{text}"),
    }
    Ok(OK)
}

fn diff(before: &Path, after: &Path) -> Result<u8, Failure> {
    let (left_source, right_source) = (read(before)?, read(after)?);
    let (left_name, right_name) = (before.display().to_string(), after.display().to_string());

    // Each file's diagnostics are emitted with that file, before the
    // next one is touched. Collecting them at the end lost the left
    // file's warnings whenever the right file failed to parse — the `?`
    // returned first — so a run that reported nothing about a.jed could
    // not be told from a run where a.jed was clean.
    let parse_one = |source: &str, name: &str| {
        let result = parse_with_mode(source, FileId(0), ParserMode::PreserveUnknown);
        let bundle = match &result {
            Ok(parsed) => &parsed.diagnostics,
            Err(bundle) => bundle,
        };
        eprint!("{}", render::bundle(bundle, name, source));
        result.map_err(|_| Failure::trouble(format!("{name}: not a valid JEDEC file")))
    };

    let left = parse_one(&left_source, &left_name)?.file;
    let right = parse_one(&right_source, &right_name)?.file;

    let delta = decpld_jedec::diff(&left, &right);
    if delta.is_empty() {
        println!("{left_name} and {right_name} describe the same device");
        return Ok(OK);
    }

    print!("{}", render_diff(&delta, &left_name, &right_name));
    Ok(FINDINGS)
}

/// One-line summary of a file that parsed. Pure, so it is testable
/// without spawning a process (CLAUDE.md: logic that can be tested
/// without going through the CLI must not live inside a CLI function).
fn summarise(name: &str, file: &decpld_jedec::JedecFile) -> String {
    format!(
        "{name}: ok — {} fuses, default {}, checksum {:04X}",
        file.fuses.len(),
        describe_default(file.default_fuse),
        file.computed_fuse_checksum()
    )
}

/// Render a diff. Pure, so the formatting is testable without files.
fn render_diff(delta: &JedecDiff, before: &str, after: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "--- {before}\n+++ {after}");

    if let Some((a, b)) = &delta.design_specification {
        let _ = writeln!(out, "design specification: {a:?} -> {b:?}");
    }
    if let Some((a, b)) = delta.fuse_count {
        let _ = writeln!(out, "fuse count: {a} -> {b}");
        let _ = writeln!(
            out,
            "  (fuse states not compared: fuse N of a {a}-fuse device is not fuse N of a {b}-fuse one)"
        );
    }
    if let Some((a, b)) = delta.default_fuse {
        let _ =
            writeln!(out, "default fuse state: {} -> {}", describe_default(a), describe_default(b));
    }
    if let Some((a, b)) = delta.security {
        let _ =
            writeln!(out, "security fuse: {} -> {}", describe_security(a), describe_security(b));
    }
    if let Some((a, b)) = &delta.notes {
        let _ = writeln!(out, "notes: {a:?} -> {b:?}");
    }
    if let Some((a, b)) = &delta.unknown_fields {
        let _ = writeln!(out, "unmodelled fields: {a:?} -> {b:?}");
    }
    for fuse in &delta.fuses {
        let _ = writeln!(
            out,
            "fuse {}: {} -> {}",
            fuse.index,
            u8::from(fuse.before),
            u8::from(fuse.after)
        );
    }
    if !delta.fuses.is_empty() {
        let _ = writeln!(out, "{} fuse(s) differ", delta.fuses.len());
    }
    out
}

/// How a file's `F` field reads in a report.
///
/// "absent" rather than "0", because a file with no `F` field is one
/// where every fuse state is stated explicitly — a different claim from
/// "unlisted fuses are 0", and the distinction #20 exists to preserve.
fn describe_default(state: Option<bool>) -> &'static str {
    match state {
        None => "absent (every fuse stated)",
        Some(false) => "0",
        Some(true) => "1",
    }
}

fn describe_security(state: Option<bool>) -> &'static str {
    match state {
        None => "absent",
        Some(false) => "0",
        Some(true) => "1",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use decpld_jedec::FuseDelta;

    #[test]
    fn a_fuse_delta_names_the_fuse_and_both_states() {
        let delta = JedecDiff {
            fuses: vec![FuseDelta { index: 42, before: false, after: true }],
            ..JedecDiff::default()
        };
        let out = render_diff(&delta, "a.jed", "b.jed");
        assert!(out.contains("fuse 42: 0 -> 1"), "{out}");
        assert!(out.contains("1 fuse(s) differ"), "{out}");
    }

    #[test]
    fn a_fuse_count_difference_explains_why_fuses_are_not_compared() {
        // Silence here would read as "the fuses are identical", which is
        // the opposite of what a suppressed comparison means.
        let delta = JedecDiff { fuse_count: Some((16, 32)), ..JedecDiff::default() };
        let out = render_diff(&delta, "a.jed", "b.jed");
        assert!(out.contains("fuse count: 16 -> 32"), "{out}");
        assert!(out.contains("not compared"), "{out}");
    }

    #[test]
    fn the_summary_reports_count_default_and_checksum() {
        // `summarise` was extracted as a pure function precisely so it
        // could be tested without spawning a process, and then was not
        // tested. A present default is printed as the digit the `F`
        // field spells it with; an absent one says so in words.
        let file =
            decpld_jedec::parse("\x02h*QF16*F1*L0 0*\x030000", FileId(0)).expect("parses").file;
        let out = summarise("d.jed", &file);
        assert!(out.starts_with("d.jed: ok — 16 fuses, default 1, checksum "), "{out}");
        assert!(out.ends_with(&format!("{:04X}", file.computed_fuse_checksum())), "{out}");
    }

    #[test]
    fn an_absent_default_fuse_state_is_not_described_as_zero() {
        // The whole point of #20: a file with no F field says "every
        // fuse state is stated here", which is a different claim from
        // "unlisted fuses are 0". A report that printed `0` for both
        // would undo the distinction at the last step.
        let delta = JedecDiff { default_fuse: Some((None, Some(false))), ..JedecDiff::default() };
        let out = render_diff(&delta, "a.jed", "b.jed");
        assert!(out.contains("default fuse state: absent"), "{out}");
        assert!(!out.contains("default fuse state: 0 ->"), "{out}");
    }

    #[test]
    fn the_summary_says_absent_when_there_is_no_f_field() {
        let file = decpld_jedec::parse("\x02h*QF8*L0 10110001*\x030000", FileId(0))
            .expect("complete coverage, so no F field is needed")
            .file;
        let out = summarise("d.jed", &file);
        assert!(out.contains("default absent"), "{out}");
    }

    #[test]
    fn an_absent_security_bit_is_described_as_absent_not_as_zero() {
        let delta = JedecDiff { security: Some((None, Some(false))), ..JedecDiff::default() };
        let out = render_diff(&delta, "a.jed", "b.jed");
        assert!(out.contains("security fuse: absent -> 0"), "{out}");
    }
}
