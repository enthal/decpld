//! The ATF22V10C's packages.
//!
//! Pin *roles* are the datasheet's to state; pin *behaviour* is measured.
//! Both are cited per claim below.

use crate::geometry::{Atf22v10Geometry, MacrocellIndex};
use decpld_device::{
    ClockResourceId, InputResourceId, PackageId, PackagePin, PackageSpec, PadId, PinNumber,
    PowerRail,
};

/// The DIP-24 package.
///
/// `PackageId(0)` because DIP-24 is this device's first and, for now,
/// only modelled package.
///
/// `atf22v10c-datasheet` Figure 2-2 is headed "DIP/SOIC", so the
/// 24-pin SOIC shares this pinout — but it is not modelled here, and
/// neither are TSSOP-24 (Figure 2-1) or PLCC/LCC-28 (Figure 2-3, a
/// visibly different mapping with four no-connect pins). Adding a
/// package means reading its figure, not assuming a family
/// resemblance.
pub const DIP24: PackageId = PackageId(0);

/// The single global clock.
///
/// Evidence: the device has one clock pin (`atf22v10c-datasheet`
/// Table 2-1), and there is no per-macrocell clock select to encode —
/// the architecture region is exactly two fuses per macrocell and both
/// are accounted for as polarity and mode (`arch-comb-high`,
/// `arch-comb-low`, `arch-reg-high`, `arch-reg-low`).
pub const GLOBAL_CLOCK: ClockResourceId = ClockResourceId(0);

/// The ATF22V10C in a 24-pin DIP.
///
/// Evidence for the pin roles: `atf22v10c-datasheet` Table 2-1 "Pin
/// Configurations" and Figure 2-2 "DIP/SOIC", which give
///
/// ```text
/// 1..12    CLK/IN  IN  IN  IN/PD  IN  IN  IN  IN  IN  IN  IN  GND
/// 24..13   VCC  I/O  I/O  I/O  I/O  I/O  I/O  I/O  I/O  I/O  I/O  IN
/// ```
///
/// with Table 2-1's legend defining CLK as Clock, IN as Logic Inputs,
/// I/O as Bi-directional Buffers, GND as Ground, VCC as +5V Supply, and
/// PD as Power-down.
///
/// Evidence for the behaviour behind those names:
///
/// - every input pin's array source, from the `in*` sweep;
/// - every pad's macrocell, from `mc14` … `mc23`;
/// - pin 1 serving as clock *and* array input simultaneously, from
///   `clk-shared`;
/// - pins 12 and 24 being unusable, from `pwr12` and `pwr24`, which
///   WinCUPL refuses with "invalid input" rather than compiling.
///
/// # Panics
///
/// Never. The map is a constant with no repeated resource, so
/// `PackageSpec::new` cannot reject it; the `expect` records that as an
/// invariant rather than a hope, and the test suite builds this package
/// on every run.
#[must_use]
pub fn dip24() -> PackageSpec {
    let geometry = Atf22v10Geometry;
    let mut pins: Vec<(PinNumber, PackagePin)> = Vec::with_capacity(24);

    // Pin 1 is CLK/IN: both roles at once, not a choice between them.
    // `clk-shared` drives a registered output's data term from pin 1,
    // which needs the clock and the array input in the same design.
    //
    // The input resource id is the array source index, here 0. Reusing
    // the source index rather than allocating a parallel numbering means
    // the package and the matrix cannot drift apart: there is one
    // number, not two that must agree.
    //
    // The input id is read from the geometry rather than written out as
    // a literal. Hard-coding it while the loop below excluded pin 1 by
    // *pin number* stated "pin 1 is array source 0" twice, in two
    // different forms, with nothing keeping them equal.
    let clock_pin = PinNumber(1);
    let clock_source =
        geometry.source_of_pin(clock_pin).expect("pin 1 is an array input source").index;
    pins.push((
        clock_pin,
        PackagePin::SharedClockInput { clock: GLOBAL_CLOCK, input: InputResourceId(clock_source) },
    ));

    // Pins 2..=11 and 13 are logic inputs; 12 is GND and 24 is VCC.
    //
    // Pin 4 is IN/PD. The datasheet §9 makes the power-down role
    // conditional — "when the power-down feature is not specified in the
    // design file, the IN/PD pin will be configured as a regular logic
    // input" — and a design that does use it "may not use the PD pin
    // logic array input". Power-down mode is not modelled yet, so pin 4
    // is a plain input, which is what the datasheet says it is in that
    // case. Modelling the other case means a second package or a
    // mode-dependent role, and inventing either now would be a guess
    // about a feature nothing can yet select.
    for source in geometry.sources() {
        if let crate::geometry::SourceKind::Input(pin) = source.kind
            && pin != clock_pin
        {
            pins.push((pin, PackagePin::DedicatedInput(InputResourceId(source.index))));
        }
    }

    pins.push((PinNumber(12), PackagePin::Power(PowerRail::Ground)));
    pins.push((PinNumber(24), PackagePin::Power(PowerRail::Supply)));

    // Pins 14..=23 are the ten macrocell I/O cells. The pad index is the
    // macrocell index, so "which pin is macrocell 3 on" has one answer
    // reached two ways rather than two answers that must be kept equal.
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let pin = geometry.macrocell_pin(MacrocellIndex(index)).expect("index is below MACROCELLS");
        pins.push((pin, PackagePin::Pad(PadId(index))));
    }

    PackageSpec::new(DIP24, "DIP24", pins).expect("the DIP-24 map assigns each resource once")
}
