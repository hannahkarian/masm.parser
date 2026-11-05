// Unit tests: `cargo test`
use crate::{masm, verilog};

// parse + check + print; Err(messages) if parsing or checking fails
fn generate(text: &str) -> Result<(String, Vec<String>), Vec<String>> {
    let program = masm::ProgramParser::new()
        .parse(text)
        .map_err(|e| vec![e.to_string()])?;
    let ctx = verilog::Context::new(&program, "Main", "clock")?;
    let warnings = ctx.warnings.clone();
    Ok((verilog::render(&program, &ctx), warnings))
}

// wrap a few lines in a .prog/.proc header
fn prog(body: &str) -> String {
    format!(".prog:\n.proc main:\n{body}\n")
}

fn errors(body: &str) -> Vec<String> {
    generate(&prog(body)).expect_err("expected an error")
}

fn verilog_of(body: &str) -> String {
    generate(&prog(body)).expect("expected no error").0
}

// The committed generated.sv must be exactly what the tool produces for stepper.masm,
// so the file EQY checks is never a hand-edited one.
#[test]
fn stepper_matches_committed_generated_sv() {
    let program = masm::ProgramParser::new()
        .parse(include_str!("../stepper.masm"))
        .unwrap();
    let mut ctx = verilog::Context::new(&program, "Main", "clock").unwrap();
    ctx.source = Some("stepper.masm".to_string());
    assert!(ctx.warnings.is_empty(), "{:?}", ctx.warnings);
    assert_eq!(
        verilog::render(&program, &ctx),
        include_str!("../generated.sv"),
        "generated.sv is out of date: run `cargo run -- stepper.masm`"
    );
}

#[test]
fn mux_puts_true_value_first() {
    // MASM: MUX rd, sel, false, true
    let v = verilog_of(".wire s 1\n.wire f 4\n.wire t 4\n.wire d 4\nMUX d, s, f, t;");
    assert!(v.contains("assign d = s ? t : f;"), "{v}");
}

#[test]
fn padzero_adds_the_right_number_of_zeros() {
    let v = verilog_of(".const c 2 0b11\n.wire d 4\nPADZERO d, c, 4;");
    assert!(v.contains("assign d = {{2{1'b0}}, c};"), "{v}");
}

#[test]
fn add_and_seq() {
    let v = verilog_of(".wire a 4\n.wire b 4\n.wire s 4\n.wire e 1\nADD s, a, b;\nSEQ e, a, b;");
    assert!(v.contains("assign s = a + b;"), "{v}");
    assert!(v.contains("assign e = a == b;"), "{v}");
}

#[test]
fn register_is_updated_on_the_clock() {
    let v = verilog_of(".reg r 4 .input q 0b0101 .output d\nMOV d, q;");
    assert!(v.contains("reg [3:0] q = 4'b0101;"), "{v}");
    assert!(v.contains("wire [3:0] d;"), "{v}");
    assert!(v.contains("q <= d;"), "{v}");
}

#[test]
fn finish_is_guarded_for_synthesis() {
    let v = verilog_of(".wire c 1\n(0) FINISH c;");
    let guard = v.find("`ifndef SYNTHESIS").expect("no guard");
    let finish = v.find("$finish").unwrap();
    let endif = v.find("`endif").expect("no endif");
    assert!(guard < finish && finish < endif, "{v}");
}

#[test]
fn no_guard_without_finish() {
    let v = verilog_of(".wire a 1\n.wire b 1\nMOV b, a;");
    assert!(!v.contains("SYNTHESIS"), "{v}");
}

#[test]
fn debug_symbol_becomes_a_kept_alias() {
    let v = verilog_of("@DEBUGSYMBOL [ symbol = \"total\" ]\n.wire ys_w3 4\n.wire a 4\nMOV ys_w3, a;");
    assert!(v.contains("(* keep *) wire [3:0] total = ys_w3;"), "{v}");
}

#[test]
fn debug_symbol_on_a_reg_names_its_current_value() {
    let v = verilog_of("@DEBUGSYMBOL [ symbol = \"cnt\" ]\n.reg r 2 .input q 0b0 .output d\nMOV d, q;");
    assert!(v.contains("(* keep *) wire [1:0] cnt = q;"), "{v}");
}

#[test]
fn annotations_only_apply_to_the_next_def() {
    // the SOURCEINFO in between does not break the link, the next .wire gets nothing
    let v = verilog_of(
        "@DEBUGSYMBOL [ symbol = \"x\" ]\n@SOURCEINFO [ file = \"a.sv:1\" ]\n.wire w1 1\n.wire w2 1\nMOV w1, w2;",
    );
    assert!(v.contains("(* keep *) wire x = w1;"), "{v}");
    assert_eq!(v.matches("(* keep *)").count(), 1, "{v}");
}

#[test]
fn bad_debug_symbols_are_skipped_with_a_warning() {
    let body = "@DEBUGSYMBOL [ symbol = \"top.sub\" ]\n.wire a 1\n\
                @DEBUGSYMBOL [ symbol = \"b\" ]\n.wire b 1\n\
                @DEBUGSYMBOL [ symbol = \"reg\" ]\n.wire c 1\n\
                @DEBUGSYMBOL [ symbol = \"x\" ]\n.wire d 1\n\
                @DEBUGSYMBOL [ symbol = \"x\" ]\n.wire e 1\n\
                MOV a, b;";
    let (v, warnings) = generate(&prog(body)).unwrap();
    assert_eq!(v.matches("(* keep *)").count(), 1, "{v}"); // only the first `x`
    assert_eq!(warnings.len(), 4, "{warnings:?}");
}

#[test]
fn duplicate_declaration_is_an_error() {
    let e = errors(".wire a 1\n.wire a 1");
    assert!(e.iter().any(|m| m.contains("declared more than once")), "{e:?}");
}

#[test]
fn two_drivers_is_an_error() {
    let e = errors(".wire a 1\n.wire b 1\nMOV a, b;\nMOV a, b;");
    assert!(e.iter().any(|m| m.contains("more than one instruction")), "{e:?}");
}

#[test]
fn undeclared_name_is_an_error() {
    let e = errors(".wire a 1\nMOV a, nope;");
    assert!(e.iter().any(|m| m.contains("`nope` is used but never declared")), "{e:?}");
}

#[test]
fn writing_a_constant_or_register_output_is_an_error() {
    let e = errors(".const c 1 0b1\n.wire a 1\nMOV c, a;");
    assert!(e.iter().any(|m| m.contains("cannot be the destination")), "{e:?}");
    let e = errors(".reg r 1 .input q .output d\n.wire a 1\nMOV q, a;");
    assert!(e.iter().any(|m| m.contains("cannot be the destination")), "{e:?}");
}

#[test]
fn constant_too_wide_for_its_width_is_an_error() {
    let e = errors(".const c 2 0b111");
    assert!(e.iter().any(|m| m.contains("does not fit in 2 bits")), "{e:?}");
    // the largest value that fits is fine, also for 64 bits
    verilog_of(".const c 2 0b11");
    verilog_of(".const c 64 18446744073709551615");
}

#[test]
fn too_big_numbers_are_parse_errors_not_panics() {
    let e = errors(".const c 65 18446744073709551616");
    assert!(e[0].contains("does not fit in 64 bits"), "{e:?}");
    let e = errors(".wire w 4294967296");
    assert!(e[0].contains("too large"), "{e:?}");
}

#[test]
fn def_outside_proc_is_an_error_not_a_panic() {
    let e = generate(".prog:\n.wire a 1\n").expect_err("expected an error");
    assert!(e[0].contains("outside .proc"), "{e:?}");
}

#[test]
fn width_mismatch_is_a_warning() {
    let (_, w) = generate(&prog(".wire a 4\n.wire b 2\nMOV a, b;")).unwrap();
    assert!(w.iter().any(|m| m.contains("MOV operands differ")), "{w:?}");
}
