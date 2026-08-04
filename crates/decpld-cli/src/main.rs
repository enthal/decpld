//! The `decpld` command-line driver.
//!
//! Placeholder. The real command surface is specified in SPEC.md
//! Part VIII (`build`, `check`, `fmt`, `sim`, `report`, `jed …`,
//! `oracle …`, `program`); it arrives with the milestones that give
//! those commands something to do, starting with M0 (JEDEC foundation)
//! and the `decpld jed` family. Argument parsing is deliberately not
//! wired up yet — the dependency and its shape belong in the PR that
//! introduces the first real subcommand, not here.

fn main() {
    println!("{}", banner());
}

/// The pre-implementation banner.
///
/// Kept as a pure function so the binary's one behavior is testable
/// without spawning a process — the same reflex that later keeps
/// fitting and encoding logic out of `main`.
fn banner() -> String {
    format!(
        "decpld {version} — pre-implementation.\n\
         \n\
         The compiler is specified in SPEC.md and built milestone by milestone;\n\
         see PLAN.md for what has landed. No commands are wired up yet.\n\
         \n\
         {repository}",
        version = env!("CARGO_PKG_VERSION"),
        repository = env!("CARGO_PKG_REPOSITORY"),
    )
}

#[cfg(test)]
mod tests {
    use super::banner;

    #[test]
    fn banner_reports_the_crate_version_and_repository() {
        let banner = banner();
        assert!(
            banner.contains(env!("CARGO_PKG_VERSION")),
            "banner should name the version it was built from: {banner}"
        );
        assert!(
            banner.contains("https://github.com/enthal/decpld"),
            "banner should point at the project: {banner}"
        );
    }

    #[test]
    fn banner_does_not_claim_working_commands() {
        // Guards against the placeholder outliving its honesty: when a
        // real command surface lands, this test and the banner go away
        // together rather than the banner quietly over-promising.
        assert!(banner().contains("pre-implementation"));
    }
}
