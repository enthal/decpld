//! The ATF22V10C DIP-24 package, asserted against the datasheet and the
//! oracle.
//!
//! Pin *roles* come from the datasheet, which is the authority for what
//! a pin is called and wired to. Pin *behaviour* — which array column a
//! pin reaches, whether it can be a clock and an input at once — comes
//! from experiments, because that is what the compiler acts on and a
//! pinout diagram does not say it.

use decpld_atf22v10::*;
use decpld_device::{ClockResourceId, InputResourceId, PackagePin, PadId, PinNumber, PowerRail};

const G: Atf22v10Geometry = Atf22v10Geometry;

#[test]
fn the_dip24_pinout_matches_the_datasheet() {
    // Evidence: `atf22v10c-datasheet` Table 2-1 "Pin Configurations" and
    // Figure 2-2 "DIP/SOIC", which give, top-left down and top-right
    // down:
    //
    //   1..12   CLK/IN  IN  IN  IN/PD  IN  IN  IN  IN  IN  IN  IN  GND
    //   24..13  VCC  I/O x10  IN
    //
    // Table 2-1's legend defines the names: CLK = Clock, IN = Logic
    // Inputs, I/O = Bi-directional Buffers, GND = Ground, VCC = +5V
    // Supply, PD = Power-down.
    let package = dip24();

    // Pin 1 is CLK/IN — one pin, both roles. See the dedicated test
    // below for the measurement behind the "and" rather than "or".
    assert!(
        matches!(package.pin(PinNumber(1)), Some(PackagePin::SharedClockInput { .. })),
        "pin 1 is CLK/IN"
    );

    // Pins 2..11 and 13 are logic inputs. Pin 4 is IN/PD: the datasheet
    // §9 says the PD pin "acts as the power-down pin (Pin 4 on the
    // DIP/SOIC packages)" only when power-down mode is enabled in the
    // design file, and that "designs using the power-down pin may not
    // use the PD pin logic array input". Power-down mode is not yet
    // modelled, so pin 4 is a plain input here — which is what the
    // datasheet says it is when the feature is not specified.
    for pin in [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13] {
        assert!(
            matches!(package.pin(PinNumber(pin)), Some(PackagePin::DedicatedInput(_))),
            "pin {pin} is a logic input"
        );
    }

    assert_eq!(package.pin(PinNumber(12)), Some(PackagePin::Power(PowerRail::Ground)));
    assert_eq!(package.pin(PinNumber(24)), Some(PackagePin::Power(PowerRail::Supply)));

    for pin in 14..=23 {
        assert!(
            matches!(package.pin(PinNumber(pin)), Some(PackagePin::Pad(_))),
            "pin {pin} is a bi-directional I/O"
        );
    }

    // Twenty-four pins, no more and no less.
    assert_eq!(package.pins().count(), 24);
    assert_eq!(package.pin(PinNumber(0)), None);
    assert_eq!(package.pin(PinNumber(25)), None);
}

#[test]
fn the_power_pins_are_the_only_unusable_pins() {
    // Measured, not merely read off the pinout: WinCUPL refuses a design
    // declaring `PIN 12` or `PIN 24` as a signal, reporting "invalid
    // input" and producing no JEDEC (experiments `pwr12`, `pwr24` —
    // expected-failure designs). Every other pin appears in some
    // experiment that does compile.
    let package = dip24();
    for (pin, role) in package.pins() {
        let expected = pin != PinNumber(12) && pin != PinNumber(24);
        assert_eq!(role.is_usable(), expected, "pin {}", pin.0);
    }
}

#[test]
fn pin_1_is_a_clock_and_an_array_input_at_the_same_time() {
    // Evidence: experiment `clk-shared`. A registered output on pin 23
    // driven from pin 1 compiles, and leaves fuse 88 intact — row 2,
    // column 0, pin 1's true column — while the register is clocked.
    // The architecture pair reads S0 = 1, S1 = 0: active high,
    // registered.
    //
    // The distinction matters. `PackagePin::Clock` would mean a design
    // must choose; `SharedClockInput` means it need not, and a fitter
    // that got this wrong would reject a legal design with a resource
    // error nobody could act on.
    let package = dip24();
    let Some(PackagePin::SharedClockInput { clock, input }) = package.pin(PinNumber(1)) else {
        panic!("pin 1 must carry both a clock and an input");
    };
    assert_eq!(package.pin_of_clock(clock), Some(PinNumber(1)));
    assert_eq!(package.pin_of_input(input), Some(PinNumber(1)));

    // Pin 1 is the only clock on this device: the architecture bits are
    // two per macrocell and both are accounted for (polarity and mode),
    // so no per-macrocell clock select exists to encode.
    let clocks: Vec<_> =
        package.pins().filter_map(|(pin, role)| role.clock().map(|_| pin)).collect();
    assert_eq!(clocks, [PinNumber(1)]);
}

#[test]
fn every_pad_belongs_to_the_macrocell_that_drives_its_pin() {
    // Evidence: `mc14` … `mc23`, one design per macrocell. The pad index
    // is the macrocell index, so a fitter placing an output on a
    // macrocell and a report naming a pin cannot disagree.
    let package = dip24();
    for index in 0..Atf22v10Geometry::MACROCELLS {
        let macrocell = MacrocellIndex(index);
        let pin = G.macrocell_pin(macrocell).expect("a macrocell drives a pin");
        assert_eq!(package.pin(pin), Some(PackagePin::Pad(PadId(index))), "macrocell {index}");
        assert_eq!(package.pin_of_pad(PadId(index)), Some(pin));
    }
}

#[test]
fn every_input_pin_carries_its_measured_array_source() {
    // Evidence: the `in*` sweep. The input resource id IS the array
    // source index, so the package and the matrix cannot drift: there is
    // one number, not two that must agree.
    let package = dip24();
    for (pin, role) in package.pins() {
        let Some(input) = role.dedicated_input() else { continue };
        let source = G.source(input.0).unwrap_or_else(|| panic!("no source {}", input.0));
        assert_eq!(
            source.kind,
            SourceKind::Input(pin),
            "pin {} claims array source {}",
            pin.0,
            input.0
        );
    }
}

#[test]
fn an_io_pin_used_only_as_an_input_reaches_its_own_feedback_column() {
    // Evidence: experiments `ioin14` … `ioin23`. Each drives a *different*
    // I/O pin from an undriven one, and the literal lands on the
    // undriven pin's macrocell feedback column:
    //
    //   pin 14 15 16 17 18 19 20 21 22 23
    //   col 38 34 30 26 22 18 14 10  6  2
    //
    // So an I/O pin reaching the array as an input does so through its
    // own macrocell's feedback path — there is no separate input
    // resource for it, which is why `PackagePin::Pad` carries no
    // `InputResourceId` and `PackagePin::input()` returns `None` for a
    // pad.
    //
    // All ten measured rather than generalised from one: "an I/O pin can
    // be an input" is a claim about ten pins, and the feedback column
    // map runs opposite to the pin numbering, which is exactly the shape
    // where checking one end proves nothing about the other.
    let expected: [(u8, u32); 10] = [
        (14, 38),
        (15, 34),
        (16, 30),
        (17, 26),
        (18, 22),
        (19, 18),
        (20, 14),
        (21, 10),
        (22, 6),
        (23, 2),
    ];
    for (pin, column) in expected {
        let source = G.source_of_pin(PinNumber(pin)).unwrap_or_else(|| panic!("pin {pin}"));
        assert_eq!(source.true_column(), column, "pin {pin}");
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        assert_eq!(source.kind, SourceKind::Feedback(macrocell), "pin {pin}");
    }
}

#[test]
fn source_of_pin_answers_for_input_pins_too_and_refuses_the_rest() {
    // One lookup for "which column carries this pin", whichever kind of
    // pin it is — the question an encoder actually asks when turning a
    // literal into a fuse. Splitting it across two functions would make
    // the caller decide which to call, and the caller is exactly who
    // does not know.
    for (pin, column) in [(1u8, 0u32), (2, 4), (3, 8), (11, 40), (13, 42)] {
        let source = G.source_of_pin(PinNumber(pin)).unwrap_or_else(|| panic!("pin {pin}"));
        assert_eq!(source.true_column(), column, "pin {pin}");
        assert_eq!(source.kind, SourceKind::Input(PinNumber(pin)), "pin {pin}");
    }

    // The supply rails carry no signal, and neither does a pin the
    // package does not have.
    for pin in [0u8, 12, 24, 25, 255] {
        assert!(G.source_of_pin(PinNumber(pin)).is_none(), "pin {pin}");
    }
}

#[test]
fn every_usable_pin_reaches_the_array_and_every_source_has_a_pin() {
    // The two directions together: no usable pin is unreachable by a
    // literal, and no array source is a column with nothing wired to it.
    // A gap either way would be a resource the fitter can never use or a
    // column an encoder could address with no pin behind it.
    let package = dip24();
    for (pin, role) in package.pins() {
        assert_eq!(
            G.source_of_pin(pin).is_some(),
            role.is_usable(),
            "pin {} usable={} ",
            pin.0,
            role.is_usable()
        );
    }

    for source in G.sources() {
        let pin = match source.kind {
            SourceKind::Input(pin) => pin,
            SourceKind::Feedback(macrocell) => {
                G.macrocell_pin(macrocell).expect("a macrocell drives a pin")
            }
        };
        assert!(package.pin(pin).is_some(), "source {} names absent pin {}", source.index, pin.0);
        assert_eq!(G.source_of_pin(pin).map(|s| s.index), Some(source.index), "pin {}", pin.0);
    }
}

#[test]
fn the_clock_resource_is_not_confused_with_the_input_resource_on_pin_1() {
    // Pin 1 carries `ClockResourceId(0)` and `InputResourceId(0)`. They
    // are different types with the same inner value, which is precisely
    // the situation where a lookup keyed on the wrong one still returns
    // an answer. The types stop it at compile time; this states that the
    // values really do collide, so the test is not accidentally passing
    // because the numbers differ.
    let package = dip24();
    let Some(PackagePin::SharedClockInput { clock, input }) = package.pin(PinNumber(1)) else {
        panic!("pin 1");
    };
    assert_eq!(clock, ClockResourceId(0));
    assert_eq!(input, InputResourceId(0));
    assert_eq!(package.pin_of_clock(ClockResourceId(0)), Some(PinNumber(1)));
    assert_eq!(package.pin_of_input(InputResourceId(0)), Some(PinNumber(1)));
}
