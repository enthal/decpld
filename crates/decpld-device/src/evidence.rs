//! How well a device fact is established. SPEC.md §13.1.
//!
//! The ATF22V10 and ATF16V8 fuse maps are this project's factual
//! bedrock, and a wrong bit is invisible until hardware misbehaves. A
//! constant on its own cannot say whether it was measured, read off a
//! datasheet, copied from another implementation, or guessed — so the
//! type says it, and a threshold makes "no hypotheses in a production
//! target" a check rather than a habit.

use std::fmt;

/// How many independent witnesses stand behind a fact.
///
/// **The ordering counts witnesses; it does not rank authority.**
/// [`Self::DatasheetSpecified`] sits below
/// [`Self::DifferentiallyVerified`] because one document is one
/// witness, not because a vendor is less trustworthy than an oracle
/// run. For a fact no fuse experiment can observe — which pin is bonded
/// to ground, what a supply rail is — the datasheet is the *better*
/// witness and the only one available. A field at that level is a field
/// no *experiment* has reached, which is what the level is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EvidenceLevel {
    /// Nothing established this. Belongs in oracle-analysis code or a
    /// disabled experimental target, never in a production target.
    Hypothesis,
    /// Documents say so, and no experiment has touched it.
    ///
    /// Usually one document. It is also where a fact that **two**
    /// documents agree on has to sit, and the ladder has no rung of its
    /// own for that: the levels above assert a controlled experiment,
    /// and two implementations agreeing with each other is not one.
    /// Taking the highest rung whose prerequisites are met is the rule,
    /// so a fact with two documents and no run is `DatasheetSpecified` —
    /// see `decpld-atf22v10`'s `CapacityCrossChecked`. Adding a rung
    /// would be a change to this ladder and to SPEC.md §13.1, not
    /// something to infer from a single mapping needing it.
    DatasheetSpecified,
    /// A controlled experiment changed one thing and observed the
    /// result.
    DifferentiallyVerified,
    /// An independent implementation agrees.
    OpenSourceCrossChecked,
    /// A part was programmed and its behaviour measured. The only level
    /// that settles a convention both the encoder and the decoder
    /// share.
    HardwareVerified,
}

impl EvidenceLevel {
    /// Every level, weakest first.
    pub const ALL: [EvidenceLevel; 5] = [
        EvidenceLevel::Hypothesis,
        EvidenceLevel::DatasheetSpecified,
        EvidenceLevel::DifferentiallyVerified,
        EvidenceLevel::OpenSourceCrossChecked,
        EvidenceLevel::HardwareVerified,
    ];

    /// The weakest level a production target field may hold.
    ///
    /// One cited witness. Deliberately not higher: package pinouts and
    /// supply rails are real, citable knowledge that no fuse experiment
    /// can reach, and a threshold above this would exclude facts that
    /// have all the evidence they can ever have.
    pub const PRODUCTION_THRESHOLD: EvidenceLevel = EvidenceLevel::DatasheetSpecified;

    /// Whether a field at this level may appear in a production target.
    #[must_use]
    pub fn is_production_ready(self) -> bool {
        self >= Self::PRODUCTION_THRESHOLD
    }
}

impl fmt::Display for EvidenceLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            EvidenceLevel::Hypothesis => "hypothesis",
            EvidenceLevel::DatasheetSpecified => "datasheet-specified",
            EvidenceLevel::DifferentiallyVerified => "differentially verified",
            EvidenceLevel::OpenSourceCrossChecked => "open-source cross-checked",
            EvidenceLevel::HardwareVerified => "hardware verified",
        };
        f.write_str(name)
    }
}

/// A level and what established it.
///
/// The sources are the point. A level with nothing behind it is a claim
/// about a claim — naming the experiments is what lets a reader
/// re-derive the fact, and what lets every constant that trusted an
/// oracle run be found if the run is later shown wrong.
/// The fields are private, and that is the whole point. Public fields
/// leave a struct literal as an unchecked back door around
/// [`Evidence::established`] — `Evidence { level: HardwareVerified,
/// sources: &[] }` would compile from any crate — which puts the rule
/// back where it started, resting on callers choosing the honest
/// constructor.
///
/// ```compile_fail
/// # use decpld_device::{Evidence, EvidenceLevel};
/// let _ = Evidence { level: EvidenceLevel::HardwareVerified, sources: &[] };
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    level: EvidenceLevel,
    /// Experiment names, document sections, or implementation
    /// references — whatever a reader would go and look at.
    sources: &'static [&'static str],
}

impl Evidence {
    /// Record evidence that names at least one source.
    ///
    /// The non-emptiness is a **compile-time** property, not a checked
    /// one: the sources arrive as an array reference, so an empty list
    /// has length zero and fails the assertion below before the program
    /// runs. A runtime guard would only be as good as the callers who
    /// chose to use it, and a constructor that merely *offers* to check
    /// is decoration — the wrong state has to be unrepresentable.
    ///
    /// ```
    /// # use decpld_device::{Evidence, EvidenceLevel};
    /// let evidence = Evidence::established(EvidenceLevel::DatasheetSpecified, &["jedec-3a"]);
    /// assert_eq!(evidence.sources(), ["jedec-3a"]);
    /// ```
    ///
    /// Naming nothing does not build:
    ///
    /// ```compile_fail
    /// # use decpld_device::{Evidence, EvidenceLevel};
    /// let _ = Evidence::established(EvidenceLevel::DatasheetSpecified, &[]);
    /// ```
    #[must_use]
    pub const fn established<const N: usize>(
        level: EvidenceLevel,
        sources: &'static [&'static str; N],
    ) -> Self {
        const { assert!(N > 0, "evidence above a hypothesis must name what established it") }
        Self { level, sources }
    }

    /// Nothing has established this yet.
    ///
    /// The one level that may stand alone, because "nothing established
    /// this" is exactly what it means. Written as its own constructor so
    /// that it is a deliberate statement rather than the accident of
    /// passing an empty list.
    #[must_use]
    pub const fn hypothesis() -> Self {
        Self { level: EvidenceLevel::Hypothesis, sources: &[] }
    }

    /// How well established it is.
    #[must_use]
    pub const fn level(&self) -> EvidenceLevel {
        self.level
    }

    /// What established it.
    #[must_use]
    pub const fn sources(&self) -> &'static [&'static str] {
        self.sources
    }

    #[must_use]
    pub fn is_production_ready(&self) -> bool {
        self.level.is_production_ready()
    }

    /// What a conclusion drawn from several facts actually rests on.
    ///
    /// The **weakest** level, not an average: a mapping assembled from
    /// a measured column table and an assumed link convention is only
    /// as established as the convention. Every source is carried
    /// through, so the weak link stays findable.
    ///
    /// Combining *nothing* yields [`EvidenceLevel::Hypothesis`], not
    /// the identity a fold over `min` would give. Seeding with the
    /// strongest level is the natural way to write this and answers
    /// `HardwareVerified` for an empty iterator — maximal evidence for
    /// no facts at all, which on this project is the worst available
    /// default.
    ///
    /// A source named by two parts appears **once**, in first-seen
    /// order. Repeating it would read as two witnesses where there is
    /// one, and overcounting witnesses is the error this type exists to
    /// prevent.
    #[must_use]
    pub fn weakest(parts: impl IntoIterator<Item = Evidence>) -> CombinedEvidence {
        let mut level = None;
        let mut sources: Vec<&'static str> = Vec::new();
        for part in parts {
            level = Some(level.map_or(part.level, |held: EvidenceLevel| held.min(part.level)));
            for source in part.sources {
                if !sources.contains(source) {
                    sources.push(source);
                }
            }
        }
        CombinedEvidence { level: level.unwrap_or(EvidenceLevel::Hypothesis), sources }
    }
}

/// Evidence drawn from several facts, owning its collected sources.
///
/// Private fields for the same reason [`Evidence`]'s are: a combination
/// is a conclusion about what several facts jointly support, and a
/// literal that asserts one directly has combined nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CombinedEvidence {
    level: EvidenceLevel,
    sources: Vec<&'static str>,
}

impl CombinedEvidence {
    /// How well established the conclusion is: the weakest part.
    #[must_use]
    pub fn level(&self) -> EvidenceLevel {
        self.level
    }

    /// Every source of every part, deduplicated, in first-seen order.
    #[must_use]
    pub fn sources(&self) -> &[&'static str] {
        &self.sources
    }

    #[must_use]
    pub fn is_production_ready(&self) -> bool {
        self.level.is_production_ready()
    }
}
