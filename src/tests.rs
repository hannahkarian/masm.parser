use crate::{masm, verilog};

/// Parse, check and print: `Ok((verilog, warnings))` or `Err(messages)`.
fn generate(text: &str) -> Result<(String, Vec<String>), Vec<String>> {
    let program = masm::ProgramParser::new()
        .parse(text)
        .map_err(|e| vec![e.to_string()])?;
    let ctx = verilog::Context::new(&program, "Main", "clock")?;
    let warnings = ctx.warnings.clone();
    Ok((verilog::render(&program, &ctx), warnings))
}

/// Wraps a few lines in a `.prog`/`.proc` header.
fn prog(body: &str) -> String {
    format!(".prog:\n.proc main:\n{body}\n")
}

fn errors(body: &str) -> Vec<String> {
    generate(&prog(body)).expect_err("expected an error")
}

fn verilog_of(body: &str) -> String {
    generate(&prog(body)).expect("expected no error").0
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
fn plain_alu_operations() {
    let ops = [
        ("ADD", "+"),
        ("SUB", "-"),
        ("MUL", "*"),
        ("AND", "&"),
        ("OR", "|"),
        ("XOR", "^"),
    ];
    for (masm, verilog) in ops {
        let v = verilog_of(&format!(".wire a 4\n.wire b 4\n.wire d 4\n{masm} d, a, b;"));
        // operand order matters for SUB
        assert!(
            v.contains(&format!("assign d = a {verilog} b;")),
            "{masm}: {v}"
        );

        let (_, w) = generate(&prog(&format!(
            ".wire a 4\n.wire b 2\n.wire d 4\n{masm} d, a, b;"
        )))
        .unwrap();
        let expected = format!("{masm} operands differ in width");
        assert!(w.iter().any(|m| m.contains(&expected)), "{masm}: {w:?}");
    }
}

#[test]
fn alu_operations_need_all_three_operands() {
    for body in ["SUB d, a;", "XOR d, a, b, a;", "MUL d a b;"] {
        let body = format!(".wire a 4\n.wire b 4\n.wire d 4\n{body}");
        assert!(generate(&prog(&body)).is_err(), "{body}");
    }
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
    let v =
        verilog_of("@DEBUGSYMBOL [ symbol = \"total\" ]\n.wire ys_w3 4\n.wire a 4\nMOV ys_w3, a;");
    assert!(v.contains("(* keep *) wire [3:0] total = ys_w3;"), "{v}");
}

#[test]
fn debug_symbol_on_a_reg_names_its_current_value() {
    let v =
        verilog_of("@DEBUGSYMBOL [ symbol = \"cnt\" ]\n.reg r 2 .input q 0b0 .output d\nMOV d, q;");
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
    assert!(
        e.iter().any(|m| m.contains("declared more than once")),
        "{e:?}"
    );
}

#[test]
fn two_drivers_is_an_error() {
    let e = errors(".wire a 1\n.wire b 1\nMOV a, b;\nMOV a, b;");
    assert!(
        e.iter().any(|m| m.contains("more than one instruction")),
        "{e:?}"
    );
}

#[test]
fn undeclared_name_is_an_error() {
    let e = errors(".wire a 1\nMOV a, nope;");
    assert!(
        e.iter()
            .any(|m| m.contains("`nope` is used but never declared")),
        "{e:?}"
    );
}

#[test]
fn writing_a_constant_or_register_output_is_an_error() {
    let e = errors(".const c 1 0b1\n.wire a 1\nMOV c, a;");
    assert!(
        e.iter().any(|m| m.contains("cannot be the destination")),
        "{e:?}"
    );
    let e = errors(".reg r 1 .input q .output d\n.wire a 1\nMOV q, a;");
    assert!(
        e.iter().any(|m| m.contains("cannot be the destination")),
        "{e:?}"
    );
}

#[test]
fn constant_too_wide_for_its_width_is_an_error() {
    let e = errors(".const c 2 0b111");
    assert!(
        e.iter().any(|m| m.contains("does not fit in 2 bits")),
        "{e:?}"
    );
    // the largest value that fits is fine, also for 64 bits
    verilog_of(".const c 2 0b11");
    verilog_of(".const c 64 18446744073709551615");
}

#[test]
fn constants_and_init_values_can_be_wider_than_64_bits() {
    // 2^64 needs 65 bits
    let v = verilog_of(".const c 65 18446744073709551616");
    assert!(
        v.contains(&format!("localparam [64:0] c = 65'b1{};", "0".repeat(64))),
        "{v}"
    );
    let e = errors(".const c 64 18446744073709551616");
    assert!(
        e.iter().any(|m| m.contains("does not fit in 64 bits")),
        "{e:?}"
    );

    let v = verilog_of(&format!(
        ".reg r 70 .input q 0b1{} .output d\nMOV d, q;",
        "0".repeat(69)
    ));
    assert!(
        v.contains(&format!("reg [69:0] q = 70'b1{};", "0".repeat(69))),
        "{v}"
    );
}

#[test]
fn hex_decimal_and_binary_numbers_agree() {
    let v = verilog_of(".const a 8 0xfF\n.const b 8 255\n.const c 8 0b11111111");
    assert_eq!(v.matches("= 8'b11111111;").count(), 3, "{v}");
    // also as a width
    let v = verilog_of(".wire w 0x10");
    assert!(v.contains("wire [15:0] w;"), "{v}");
}

#[test]
fn too_big_width_is_a_parse_error_not_a_panic() {
    let e = errors(".wire w 4294967296");
    assert!(e[0].contains("too large"), "{e:?}");
}

#[test]
fn annotations_take_several_string_and_integer_fields() {
    use crate::ast::AnnotationValue::{Int, Str};
    let items = masm::ItemsParser::new()
        .parse("@MEMBLOCK [block = \"m\", width = 8, capacity = 0x10]\n@MEMINIT [ file = \"m.hex\", count = 0b11, width = 8 ]\n@TRACK\n@EMPTY [ ]")
        .unwrap();
    let fields: Vec<_> = items
        .into_iter()
        .map(|item| match item {
            crate::ast::Item::Annotation(a) => (a.name, a.fields),
            other => panic!("not an annotation: {other:?}"),
        })
        .collect();
    let key = |k: &str| k.to_string();
    assert_eq!(
        fields,
        vec![
            (
                key("MEMBLOCK"),
                vec![
                    (key("block"), Str(key("m"))),
                    (key("width"), Int(8)),
                    (key("capacity"), Int(16))
                ]
            ),
            (
                key("MEMINIT"),
                vec![
                    (key("file"), Str(key("m.hex"))),
                    (key("count"), Int(3)),
                    (key("width"), Int(8))
                ]
            ),
            (key("TRACK"), vec![]),
            (key("EMPTY"), vec![]),
        ]
    );
}

#[test]
fn malformed_annotations_are_parse_errors() {
    for body in [
        "@A [ k = 1, ]\n.wire w 1",
        "@A [ k ]\n.wire w 1",
        "@A [ k = 1 j = 2 ]\n.wire w 1",
        "@A [ k = 18446744073709551616 ]\n.wire w 1",
    ] {
        assert!(generate(&prog(body)).is_err(), "{body}");
    }
}

#[test]
fn debug_symbol_is_found_among_other_fields() {
    let v = verilog_of(
        "@DEBUGSYMBOL [ width = 4, symbol = \"total\" ]\n.wire w 4\n.wire a 4\nMOV w, a;",
    );
    assert!(v.contains("(* keep *) wire [3:0] total = w;"), "{v}");
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
