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
        #[arg(long, value_enum, default_value_t = Mode::PreserveUnknown)]
        mode: Mode,
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
enum Mode {
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

impl From<Mode> for ParserMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Strict => Self::Strict,
            Mode::Compatible => Self::Compatible,
            Mode::PreserveUnknown => Self::PreserveUnknown,
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
        Command::Jed(JedCommand::Validate { file, mode }) => validate(&file, mode.into()),
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
            print!("{}", render::bundle(&parsed.diagnostics, &name, &source));
            let file = parsed.file;
            println!(
                "{name}: ok — {} fuses, default {}, checksum {:04X}",
                file.fuses.len(),
                u8::from(file.default_fuse),
                file.computed_fuse_checksum()
            );
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

    let parse_one = |source: &str, name: &str| {
        parse_with_mode(source, FileId(0), ParserMode::PreserveUnknown).map_err(|bundle| {
            eprint!("{}", render::bundle(&bundle, name, source));
            Failure::trouble(format!("{name}: not a valid JEDEC file"))
        })
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
        let _ = writeln!(out, "default fuse state: {} -> {}", u8::from(a), u8::from(b));
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
    fn an_absent_security_bit_is_described_as_absent_not_as_zero() {
        let delta = JedecDiff { security: Some((None, Some(false))), ..JedecDiff::default() };
        let out = render_diff(&delta, "a.jed", "b.jed");
        assert!(out.contains("security fuse: absent -> 0"), "{out}");
    }
}
