//! Product-term capacity and row ownership, measured.
//!
//! SPEC.md §7.4's capacity group: "one through N independent product
//! terms per macrocell; determine exact row ownership and fit
//! boundary."
//!
//! Until these ran, the capacities came from the datasheet's Figure 1-1
//! ("8 TO 16 PRODUCT TERMS") plus row blocks cross-checked against
//! Galette. That is two documents agreeing, which is worth having and
//! is not a boundary: neither says what happens at the ninth term.
//! `cap23-8` … `cap14-9` ask directly — a design WinCUPL accepts at
//! eight and refuses at nine is a measurement.
//!
//! Recorded in `targets/evidence/atf22v10-fuse-map.md`, "Capacity".

use decpld_atf22v10::*;
use decpld_device::{
    FuseId, MacrocellId, MacrocellMode, OutputPolarity, PhysicalDesign, PinNumber, PlacedCube,
    ProductTermId,
};
use decpld_logic::{Cube, Literal, Polarity};

const G: Atf22v10Geometry = Atf22v10Geometry;

/// The blown fuses of a map, as inclusive runs.
///
/// The unit the evidence document argues for: "the unit is the
/// **written extent**". Asserting individual intact fuses proves
/// nothing on this device — `encode_design` writes every unused row
/// all-intact, so `Some(false)` at an address is also what an untouched
/// row holds, and a design with no terms at all satisfies it.
fn blown_runs(fuses: &decpld_device::FuseMap, end: u32) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for fuse in 0..end {
        if fuses.get(FuseId(fuse)) != Some(true) {
            continue;
        }
        match runs.last_mut() {
            Some(last) if last.1 + 1 == fuse => last.1 = fuse,
            _ => runs.push((fuse, fuse)),
        }
    }
    runs
}

fn literal(pin: u8) -> Literal {
    let source = G.source_of_pin(PinNumber(pin)).expect("a signal pin");
    Literal::new(bool_input_of_source(source.index), Polarity::True)
}

/// A design driving `pin` from a sum of `pins`, one literal per term,
/// filling the block's data rows from the first.
fn sum_of_literals(pin: u8, pins: &[u8]) -> PhysicalDesign {
    let mut design = blank_design().expect("a blank design");
    let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let cell = design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present");
    cell.mode = MacrocellMode::Combinational;
    cell.polarity = OutputPolarity::ActiveHigh;
    cell.oe_term =
        Some(PlacedCube { row: ProductTermId(block.output_enable_row), cube: Cube::always() });
    cell.data_terms = pins
        .iter()
        .enumerate()
        .map(|(index, &source)| PlacedCube {
            row: ProductTermId(block.data_rows.start + u32::try_from(index).expect("under 16")),
            cube: Cube::new([literal(source)]),
        })
        .collect();
    design
}

#[test]
fn the_measured_blocks_are_the_ones_the_oracle_fills() {
    // Row ownership, at absolute rows, from designs that use every data
    // row a block has. `cap23-8` writes rows 1-9 and stops; `cap19-16`
    // writes 49-65; `cap14-8` writes 122-130. Each block's first row is
    // its output enable and the rest are data.
    //
    // This measures directly what was previously cross-checked against
    // Galette's `OLMC_ROWS_22V10` and `OLMC_SIZE_22V10`. Two independent
    // sources agreeing is what SPEC.md §13.1 calls
    // `OpenSourceCrossChecked`; this makes it a measurement as well.
    for (pin, first, last, data_terms) in
        [(23u8, 1u32, 9u32, 8usize), (19, 49, 65, 16), (14, 122, 130, 8)]
    {
        let macrocell = G.macrocell_of_pin(PinNumber(pin)).expect("an I/O pin");
        let block = G.row_block(macrocell).expect("a measured block");
        assert_eq!(block.output_enable_row, first, "pin {pin}'s enable row");
        assert_eq!(block.data_rows.start, first + 1, "pin {pin}'s first data row");
        assert_eq!(block.last_data_row(), Some(last), "pin {pin}'s last data row");
        assert_eq!(block.data_rows.len(), data_terms, "pin {pin}'s data-term count");
    }
}

#[test]
fn eight_terms_on_pin_23_reproduce_the_fuses_the_oracle_wrote() {
    // `cap23-8`: `o0 = i0 # i1 # ... # i7` over pins 1-8. A sum of
    // eight distinct single literals is already minimal SOP, so CUPL
    // cannot merge them — eight literals need eight product terms.
    //
    // The oracle's intact links, one per data row:
    //
    //     row 2: 88   row 3: 136  row 4: 184  row 5: 232
    //     row 6: 280  row 7: 328  row 8: 376  row 9: 424
    //
    // which are columns 0, 4, 8, 12, 16, 20, 24, 28 — pins 1 through 8.
    let design = sum_of_literals(23, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let fuses = encode_design(&design, Footprint::Gal).expect("eight terms fit");

    // The whole array as blown RUNS, the oracle's own output shape.
    // Row 1 is entirely blown — the always-enabled output-enable term,
    // no literals — and each of rows 2-9 is blown but for one intact
    // link. Nothing outside 44-439 is blown at all, which is the
    // "nothing outside the block" claim made observable.
    //
    // The eight intact addresses on their own proved nothing: every
    // unused row is written all-intact too, so a design with no data
    // terms, or one whose terms all encoded as never-true, passed.
    assert_eq!(
        blown_runs(&fuses, 5808),
        [
            (44, 87),   // row 1, output-enable term: no literals
            (89, 135),  // row 2, intact at 88  — pin 1
            (137, 183), // row 3, intact at 136 — pin 2
            (185, 231), // row 4, intact at 184 — pin 3
            (233, 279), // row 5, intact at 232 — pin 4
            (281, 327), // row 6, intact at 280 — pin 5
            (329, 375), // row 7, intact at 328 — pin 6
            (377, 423), // row 8, intact at 376 — pin 7
            (425, 439), // row 9, intact at 424 — pin 8
        ]
    );
}

#[test]
fn sixteen_terms_fit_the_widest_block_and_fill_it_exactly() {
    // `cap19-16`. Pin 19's macrocell is the widest — sixteen data rows,
    // twice pin 23's — and the asymmetry is why a fitter needs a
    // per-macrocell capacity rather than one number for the part.
    let pins = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17];
    let design = sum_of_literals(19, &pins);
    let fuses = encode_design(&design, Footprint::Gal).expect("sixteen terms fit");

    // Rows 49-65 as blown runs. Row 49 is the enable term; rows 50-61
    // hold pins 1-11 at columns 0-40 and pin 13 at 42; rows 62-65 hold
    // pins 14-17 at their FEEDBACK columns 38, 34, 30, 26 — the columns
    // the `fb*` and `ioin*` sweeps recorded, reached here by a design
    // that needed more inputs than the part has dedicated ones.
    assert_eq!(
        blown_runs(&fuses, 5808),
        [
            (2156, 2199), // row 49, output-enable term
            (2201, 2247), // row 50, intact at 2200 — pin 1,  column 0
            (2249, 2295), // row 51, intact at 2248 — pin 2
            (2297, 2343), // row 52, intact at 2296 — pin 3
            (2345, 2391), // row 53, intact at 2344 — pin 4
            (2393, 2439), // row 54, intact at 2392 — pin 5
            (2441, 2487), // row 55, intact at 2440 — pin 6
            (2489, 2535), // row 56, intact at 2488 — pin 7
            (2537, 2583), // row 57, intact at 2536 — pin 8
            (2585, 2631), // row 58, intact at 2584 — pin 9
            (2633, 2679), // row 59, intact at 2632 — pin 10
            (2681, 2725), // row 60, intact at 2680 — pin 11, column 40
            (2727, 2765), // row 61, intact at 2726 — pin 13, column 42
            (2767, 2805), // row 62, intact at 2766 — pin 14 feedback, 38
            (2807, 2845), // row 63, intact at 2806 — pin 15 feedback, 34
            (2847, 2885), // row 64, intact at 2846 — pin 16 feedback, 30
            (2887, 2903), // row 65, intact at 2886 — pin 17 feedback, 26
        ]
    );
}

#[test]
fn a_ninth_term_on_an_eight_term_macrocell_is_refused() {
    // The boundary, and the point of the whole group. `cap23-9` is
    // `cap23-8` plus one literal, and WinCUPL turns it away — so
    // deCPLD must too, at the same place, or one of the two compilers
    // is wrong about the part.
    //
    // deCPLD refuses it as an ownership violation rather than as a
    // resource error, because at this layer a term names the row it
    // goes in: there is no ninth row for it to name.
    let macrocell = G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let mut design = sum_of_literals(23, &[1, 2, 3, 4, 5, 6, 7, 8]);
    let overflow = ProductTermId(block.data_rows.end);
    design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present")
        .data_terms
        .push(PlacedCube { row: overflow, cube: Cube::new([literal(9)]) });

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("nine terms do not fit"),
        DesignError::TermNotOwned { macrocell: MacrocellId(9), row: overflow }
    );
}

#[test]
fn a_seventeenth_term_on_the_widest_macrocell_is_refused() {
    // `cap19-17`. Measured separately from pin 23 because the
    // capacities are not uniform: a boundary confirmed at eight says
    // nothing about one at sixteen.
    let macrocell = G.macrocell_of_pin(PinNumber(19)).expect("an I/O pin");
    let block = G.row_block(macrocell).expect("a measured block");
    let pins = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14, 15, 16, 17];
    let mut design = sum_of_literals(19, &pins);
    let overflow = ProductTermId(block.data_rows.end);
    design
        .macrocells
        .iter_mut()
        .find(|cell| cell.id == MacrocellId(macrocell.0))
        .expect("present")
        .data_terms
        .push(PlacedCube { row: overflow, cube: Cube::new([literal(18)]) });

    assert_eq!(
        encode_design(&design, Footprint::Gal).expect_err("seventeen terms do not fit"),
        DesignError::TermNotOwned { macrocell: MacrocellId(5), row: overflow }
    );
}

#[test]
fn the_row_after_a_block_belongs_to_the_next_macrocell_not_to_no_one() {
    // What makes the refusals above a *capacity* boundary rather than
    // an off-by-one: row 10 is not spare, it is pin 22's output-enable
    // row. A ninth term on pin 23 would have to take another output's
    // enable, which is why the fitter cannot quietly borrow it.
    let next = G.macrocell_of_pin(PinNumber(22)).expect("an I/O pin");
    let block = G.row_block(G.macrocell_of_pin(PinNumber(23)).expect("an I/O pin")).expect("block");
    assert_eq!(G.row_block(next).expect("a block").output_enable_row, block.data_rows.end);
}

#[test]
fn the_measured_capacity_is_capacity_in_the_true_cover() {
    // The qualification that keeps `data_term_capacity` from being
    // misread. Every driven macrocell in these designs reads S0 = 1,
    // active high — `cap23-8` sets 5808/5809, `cap14-8` sets 5826/5827
    // — so WinCUPL implemented each sum directly.
    //
    // It could have inverted. `!(i0 # ... # i8)` is
    // `!i0 & !i1 & ... & !i8`, ONE product term, so a nine-input OR
    // fits an eight-term macrocell as an active-low output. CUPL does
    // not search for that: `o0 = ...` fixes the polarity.
    //
    // So eight is a bound on product TERMS, not on logic, and a fitter
    // selecting between true and complement covers (SPEC.md §3.9) will
    // fit designs the oracle refuses. Asserting the polarity here is
    // what stops the capacity tests above from being read as the
    // stronger claim.
    // Asserting `true` at 5808 alone would prove nothing: `blank_design`
    // leaves every macrocell combinational and active high, so all
    // twenty architecture fuses read 1 in every design this file
    // builds. The pair has to be made to MOVE.
    //
    // So encode the same eight-term design at each polarity and require
    // 5808 to flip while every other architecture fuse holds. That
    // pins the pair to pin 23 — under pin-ascending order the change
    // would land at 5826 — and pins the sense, which is what the
    // oracle's S0 = 1 reading means.
    let high = encode_design(&sum_of_literals(23, &[1, 2, 3, 4, 5, 6, 7, 8]), Footprint::Gal)
        .expect("eight terms fit");

    let mut design = sum_of_literals(23, &[1, 2, 3, 4, 5, 6, 7, 8]);
    design.macrocells.iter_mut().find(|cell| cell.id == MacrocellId(9)).expect("pin 23").polarity =
        OutputPolarity::ActiveLow;
    let low = encode_design(&design, Footprint::Gal).expect("polarity does not change the fit");

    assert_eq!(high.get(FuseId(5808)), Some(true), "the oracle's reading: active high");
    assert_eq!(low.get(FuseId(5808)), Some(false), "and pin 23's S0 is the FIRST pair");
    for fuse in 5809..5828 {
        assert_eq!(high.get(FuseId(fuse)), low.get(FuseId(fuse)), "fuse {fuse} must not move");
    }
    // The array is untouched by the polarity change, so the capacity
    // measurement is about rows and this one is about the pair.
    assert_eq!(blown_runs(&high, 5808), blown_runs(&low, 5808));
}
