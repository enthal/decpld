//! Packages: which physical pin carries which device resource.
//!
//! SPEC.md §4.6. This layer knows that a package maps pin numbers to
//! resources and that the mapping must be one-to-one. It does not know
//! what an ATF22V10 is.

use std::collections::BTreeMap;
use std::fmt;

/// A package variant of a device — DIP-24, PLCC-28, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageId(pub u16);

/// A physical pin, numbered as the package numbers it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PinNumber(pub u8);

impl fmt::Display for PinNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pin {}", self.0)
    }
}

/// A macrocell: one output cell of a PAL/GAL array.
///
/// Indexed by the target, which documents its own numbering — the two
/// fuse orderings on a real part frequently disagree with each other,
/// so "macrocell 3" only means something relative to a stated
/// convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacrocellId(pub u8);

impl fmt::Display for MacrocellId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "macrocell {}", self.0)
    }
}

/// An I/O cell — the pad a macrocell drives and reads back through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PadId(pub u8);

impl fmt::Display for PadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pad {}", self.0)
    }
}

/// A clock resource. A device may have one global clock or several.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockResourceId(pub u8);

impl fmt::Display for ClockResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "clock {}", self.0)
    }
}

/// An input path into the logic array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InputResourceId(pub u8);

impl fmt::Display for InputResourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "input {}", self.0)
    }
}

/// A supply rail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PowerRail {
    /// 0 V.
    Ground,
    /// The positive supply.
    Supply,
}

/// What a package pin carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PackagePin {
    /// A supply rail. Not usable by a design.
    Power(PowerRail),
    /// An input into the array, and nothing else.
    DedicatedInput(InputResourceId),
    /// A clock, and nothing else.
    Clock(ClockResourceId),
    /// A macrocell's I/O cell.
    Pad(PadId),
    /// One pin serving as both a clock and an array input, at the same
    /// time rather than as alternatives. A design may use either, or
    /// both in one equation.
    SharedClockInput { clock: ClockResourceId, input: InputResourceId },
    /// Bonded to nothing.
    NoConnect,
}

impl PackagePin {
    /// The pad this pin carries, if any.
    #[must_use]
    pub fn pad(self) -> Option<PadId> {
        match self {
            PackagePin::Pad(pad) => Some(pad),
            _ => None,
        }
    }

    /// The clock resource this pin carries, if any.
    #[must_use]
    pub fn clock(self) -> Option<ClockResourceId> {
        match self {
            PackagePin::Clock(clock) | PackagePin::SharedClockInput { clock, .. } => Some(clock),
            _ => None,
        }
    }

    /// The *dedicated* input resource this pin carries, if any.
    ///
    /// Named for what it excludes. A pad is not a dedicated input:
    /// many architectures let an undriven I/O pin feed the array, but
    /// it does so through that macrocell's own feedback path rather
    /// than through a separate input resource, so counting it here
    /// would double-count the path. The target models that, not the
    /// package.
    ///
    /// So this is **not** the question "can a design read this pin?" —
    /// a pad answers `None` here and `true` to [`Self::is_usable`].
    /// Ask the target, which knows about feedback.
    #[must_use]
    pub fn dedicated_input(self) -> Option<InputResourceId> {
        match self {
            PackagePin::DedicatedInput(input) | PackagePin::SharedClockInput { input, .. } => {
                Some(input)
            }
            _ => None,
        }
    }

    /// Whether a design may reference this pin at all.
    #[must_use]
    pub fn is_usable(self) -> bool {
        !matches!(self, PackagePin::Power(_) | PackagePin::NoConnect)
    }
}

/// A package's pin map, validated on construction.
///
/// SPEC.md §4.7 requires that "package mappings are unique". Making that
/// a construction-time check rather than an assertion somewhere means a
/// package that maps two pins to one pad cannot be built at all — the
/// invariant holds for every value of this type, which is what lets the
/// fitter treat a pad as a placement slot without re-checking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageSpec {
    id: PackageId,
    name: &'static str,
    pins_by_number: BTreeMap<PinNumber, PackagePin>,
}

/// Why a package definition was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PackageError {
    #[error("package {name} has no pins")]
    NoPins { name: &'static str },
    #[error("package {name}: {pin} is listed twice, as {first:?} and {second:?}")]
    DuplicatePin { name: &'static str, pin: PinNumber, first: PackagePin, second: PackagePin },
    #[error("package {name}: {first} and {second} both carry {pad}")]
    DuplicatePad { name: &'static str, pad: PadId, first: PinNumber, second: PinNumber },
    #[error("package {name}: {first} and {second} both carry {clock}")]
    DuplicateClock {
        name: &'static str,
        clock: ClockResourceId,
        first: PinNumber,
        second: PinNumber,
    },
    #[error("package {name}: {first} and {second} both carry {input}")]
    DuplicateInput {
        name: &'static str,
        input: InputResourceId,
        first: PinNumber,
        second: PinNumber,
    },
}

impl PackageSpec {
    /// Build a package, checking that no resource appears on two pins.
    pub fn new(
        id: PackageId,
        name: &'static str,
        pins: impl IntoIterator<Item = (PinNumber, PackagePin)>,
    ) -> Result<Self, PackageError> {
        let mut pins_by_number: BTreeMap<PinNumber, PackagePin> = BTreeMap::new();
        for (pin, role) in pins {
            if let Some(first) = pins_by_number.insert(pin, role) {
                return Err(PackageError::DuplicatePin { name, pin, first, second: role });
            }
        }
        if pins_by_number.is_empty() {
            return Err(PackageError::NoPins { name });
        }

        // One pass per resource kind, each recording the first pin that
        // claimed a resource so the diagnostic can name both offenders.
        // `BTreeMap` keeps this deterministic: the pin reported as
        // "first" is the lower-numbered one on every run.
        let mut pads: BTreeMap<PadId, PinNumber> = BTreeMap::new();
        let mut clocks: BTreeMap<ClockResourceId, PinNumber> = BTreeMap::new();
        let mut inputs: BTreeMap<InputResourceId, PinNumber> = BTreeMap::new();

        for (&pin, &role) in &pins_by_number {
            if let Some(pad) = role.pad()
                && let Some(first) = pads.insert(pad, pin)
            {
                return Err(PackageError::DuplicatePad { name, pad, first, second: pin });
            }
            if let Some(clock) = role.clock()
                && let Some(first) = clocks.insert(clock, pin)
            {
                return Err(PackageError::DuplicateClock { name, clock, first, second: pin });
            }
            if let Some(input) = role.dedicated_input()
                && let Some(first) = inputs.insert(input, pin)
            {
                return Err(PackageError::DuplicateInput { name, input, first, second: pin });
            }
        }

        Ok(Self { id, name, pins_by_number })
    }

    #[must_use]
    pub fn id(&self) -> PackageId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// What pin `number` carries, or `None` if the package has no such
    /// pin.
    #[must_use]
    pub fn pin(&self, number: PinNumber) -> Option<PackagePin> {
        self.pins_by_number.get(&number).copied()
    }

    /// Every pin, ascending by pin number.
    #[must_use = "iterating the pins has no effect on its own"]
    pub fn pins(&self) -> impl Iterator<Item = (PinNumber, PackagePin)> + '_ {
        self.pins_by_number.iter().map(|(&pin, &role)| (pin, role))
    }

    /// The pin carrying a pad. Unique by construction.
    #[must_use]
    pub fn pin_of_pad(&self, pad: PadId) -> Option<PinNumber> {
        self.pins().find(|(_, role)| role.pad() == Some(pad)).map(|(pin, _)| pin)
    }

    /// The pin carrying a clock resource. Unique by construction.
    #[must_use]
    pub fn pin_of_clock(&self, clock: ClockResourceId) -> Option<PinNumber> {
        self.pins().find(|(_, role)| role.clock() == Some(clock)).map(|(pin, _)| pin)
    }

    /// The pin carrying an array input. Unique by construction.
    #[must_use]
    pub fn pin_of_input(&self, input: InputResourceId) -> Option<PinNumber> {
        self.pins().find(|(_, role)| role.dedicated_input() == Some(input)).map(|(pin, _)| pin)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pins(roles: [(u8, PackagePin); 4]) -> impl Iterator<Item = (PinNumber, PackagePin)> {
        roles.into_iter().map(|(pin, role)| (PinNumber(pin), role))
    }

    const PAD: PackagePin = PackagePin::Pad(PadId(0));
    const GND: PackagePin = PackagePin::Power(PowerRail::Ground);

    #[test]
    fn two_pins_carrying_one_pad_is_refused() {
        // The failure this prevents is a fitter placing an output on a
        // pad and a package answering with two different pins — after
        // which the pinout in the report and the pinout on the bench
        // disagree, with nothing in between to notice.
        let error =
            PackageSpec::new(PackageId(0), "twin", pins([(1, PAD), (2, PAD), (3, GND), (4, GND)]))
                .expect_err("a pad on two pins must be refused");
        assert_eq!(
            error,
            PackageError::DuplicatePad {
                name: "twin",
                pad: PadId(0),
                first: PinNumber(1),
                second: PinNumber(2),
            }
        );
    }

    #[test]
    fn two_pins_carrying_one_clock_or_input_is_refused() {
        let clock = PackagePin::Clock(ClockResourceId(3));
        let error = PackageSpec::new(
            PackageId(0),
            "twin",
            pins([(1, clock), (2, clock), (3, GND), (4, GND)]),
        )
        .expect_err("a clock on two pins must be refused");
        assert_eq!(
            error,
            PackageError::DuplicateClock {
                name: "twin",
                clock: ClockResourceId(3),
                first: PinNumber(1),
                second: PinNumber(2),
            }
        );

        let input = PackagePin::DedicatedInput(InputResourceId(7));
        let error = PackageSpec::new(
            PackageId(0),
            "twin",
            pins([(1, input), (2, input), (3, GND), (4, GND)]),
        )
        .expect_err("an input on two pins must be refused");
        assert_eq!(
            error,
            PackageError::DuplicateInput {
                name: "twin",
                input: InputResourceId(7),
                first: PinNumber(1),
                second: PinNumber(2),
            }
        );
    }

    #[test]
    fn a_shared_clock_input_pin_collides_on_both_of_its_resources() {
        // `SharedClockInput` carries two resources, and a uniqueness
        // check that only looked at the dedicated variants would let
        // either escape. Both directions are checked because the bug is
        // asymmetric: the shared pin can collide with a dedicated one
        // whichever order they appear in.
        let shared =
            PackagePin::SharedClockInput { clock: ClockResourceId(0), input: InputResourceId(0) };
        let same_clock = PackagePin::Clock(ClockResourceId(0));
        let same_input = PackagePin::DedicatedInput(InputResourceId(0));

        assert!(
            PackageSpec::new(
                PackageId(0),
                "p",
                pins([(1, shared), (2, same_clock), (3, GND), (4, GND)])
            )
            .is_err(),
            "shared clock must collide with a dedicated clock"
        );
        assert!(
            PackageSpec::new(
                PackageId(0),
                "p",
                pins([(1, same_input), (2, shared), (3, GND), (4, GND)])
            )
            .is_err(),
            "shared input must collide with a dedicated input"
        );
    }

    #[test]
    fn distinct_resources_on_distinct_pins_are_accepted_and_round_trip() {
        let package = PackageSpec::new(
            PackageId(1),
            "ok",
            pins([
                (
                    1,
                    PackagePin::SharedClockInput {
                        clock: ClockResourceId(0),
                        input: InputResourceId(0),
                    },
                ),
                (2, PackagePin::DedicatedInput(InputResourceId(1))),
                (3, PackagePin::Pad(PadId(0))),
                (4, GND),
            ]),
        )
        .expect("a valid package");

        assert_eq!(package.pin_of_pad(PadId(0)), Some(PinNumber(3)));
        assert_eq!(package.pin_of_clock(ClockResourceId(0)), Some(PinNumber(1)));
        assert_eq!(package.pin_of_input(InputResourceId(0)), Some(PinNumber(1)));
        assert_eq!(package.pin_of_input(InputResourceId(1)), Some(PinNumber(2)));

        assert_eq!(package.pin_of_pad(PadId(9)), None);
        assert_eq!(package.pin(PinNumber(5)), None);
        assert_eq!(package.name(), "ok");
        assert_eq!(package.id(), PackageId(1));
    }

    #[test]
    fn one_pin_listed_twice_is_refused_rather_than_resolved_by_order() {
        // Collecting the pins into a `BTreeMap` accepted this: the last
        // entry won, a role was silently dropped, and the same set of
        // pins written in a different order produced a DIFFERENT
        // package. A pad losing its pin that way is invisible until a
        // fitter places on the macrocell and `pin_of_pad` answers
        // `None` -- the report-and-bench disagreement this type exists
        // to prevent.
        let other = PackagePin::Pad(PadId(1));
        let error =
            PackageSpec::new(PackageId(0), "dup", pins([(1, PAD), (1, other), (2, GND), (3, GND)]))
                .expect_err("one pin cannot hold two roles");
        assert_eq!(
            error,
            PackageError::DuplicatePin {
                name: "dup",
                pin: PinNumber(1),
                first: PAD,
                second: other,
            }
        );

        // Order-independence is the property, so state it in both
        // directions: a silent last-wins accepts either listing.
        assert!(
            PackageSpec::new(PackageId(0), "dup", pins([(1, other), (1, PAD), (2, GND), (3, GND)]))
                .is_err(),
            "the reversed listing must be refused too"
        );
    }

    #[test]
    fn a_pad_is_usable_but_is_not_a_dedicated_input() {
        // The two questions differ, and a caller reaching for
        // `dedicated_input` to answer "can a design read this pin?"
        // gets the wrong answer for every I/O pin on the device. The
        // name is what makes the gap visible at the call site.
        let pad = PackagePin::Pad(PadId(0));
        assert!(pad.is_usable());
        assert_eq!(pad.dedicated_input(), None);
    }

    #[test]
    fn a_package_with_no_pins_is_refused() {
        let error = PackageSpec::new(PackageId(0), "empty", [])
            .expect_err("an empty package is not a package");
        assert_eq!(error, PackageError::NoPins { name: "empty" });
    }

    #[test]
    fn power_and_no_connect_pins_are_not_usable() {
        assert!(!PackagePin::Power(PowerRail::Ground).is_usable());
        assert!(!PackagePin::Power(PowerRail::Supply).is_usable());
        assert!(!PackagePin::NoConnect.is_usable());
        assert!(PackagePin::Pad(PadId(0)).is_usable());
        assert!(PackagePin::Clock(ClockResourceId(0)).is_usable());
        assert!(PackagePin::DedicatedInput(InputResourceId(0)).is_usable());
    }

    #[test]
    fn pins_are_reported_in_pin_order_regardless_of_definition_order() {
        // SPEC.md §0.2.5: declaration order is never observable. A
        // package definition may list its pins in whatever order reads
        // best, and a report or a diagnostic must not change because of
        // it.
        let forwards = PackageSpec::new(
            PackageId(0),
            "p",
            pins([(1, PAD), (2, GND), (3, GND), (4, PackagePin::NoConnect)]),
        )
        .expect("valid");
        let backwards = PackageSpec::new(
            PackageId(0),
            "p",
            pins([(4, PackagePin::NoConnect), (3, GND), (2, GND), (1, PAD)]),
        )
        .expect("valid");

        assert_eq!(forwards, backwards);
        assert_eq!(forwards.pins().map(|(pin, _)| pin.0).collect::<Vec<_>>(), [1, 2, 3, 4]);
    }
}
