use std::collections::{HashMap, HashSet};

use num_bigint::BigUint;
use pretty::RcDoc;

use crate::ast::{BinaryOp, Def, Instruction, Program};

pub type Doc<'a> = RcDoc<'a, ()>;

const INDENT: isize = 4;
const LINE_WIDTH: usize = 100;

/// What a name was declared as.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Wire,
    Const,
    /// The `.input` name of a `.reg`: the value stored in the register now.
    RegCurrent,
    /// The `.output` name of a `.reg`: the value it gets on the next clock edge.
    RegNext,
}

/// The result of checking a program: everything the printers need besides the AST.
pub struct Context {
    pub top: String,
    pub clock: String,
    /// Input file name, only used in the header comment.
    pub source: Option<String>,
    pub warnings: Vec<String>,
    signals: HashMap<String, (Kind, u32)>,
    /// (original name, MASM signal) pairs from `@DEBUGSYMBOL`. They are printed as
    /// `(* keep *) wire total = ys_w3;` so that EQY can match them with the gold design.
    aliases: Vec<(String, String)>,
}

impl Context {
    /// Checks the program and builds the signal table. Errors mean no valid Verilog
    /// can be generated; warnings are collected in `Context::warnings`.
    pub fn new(program: &Program, top: &str, clock: &str) -> Result<Context, Vec<String>> {
        let mut ctx = Context {
            top: top.to_string(),
            clock: clock.to_string(),
            source: None,
            warnings: Vec::new(),
            signals: HashMap::new(),
            aliases: Vec::new(),
        };
        let mut errors = Vec::new();

        // 1. Declarations
        for process in &program.processes {
            for def in &process.defs {
                let names: Vec<(&String, Kind, u32)> = match def {
                    Def::Wire { name, width } => vec![(name, Kind::Wire, *width)],
                    Def::Const { name, width, .. } => vec![(name, Kind::Const, *width)],
                    Def::Reg {
                        width,
                        current,
                        next,
                        ..
                    } => vec![
                        (current, Kind::RegCurrent, *width),
                        (next, Kind::RegNext, *width),
                    ],
                };
                if let Def::Const { name, width, value } = def {
                    if value.bits() > u64::from(*width) {
                        errors.push(format!(
                            "constant `{name}` value {value:#b} does not fit in {width} bits"
                        ));
                    }
                }
                for (name, kind, width) in names {
                    if width == 0 {
                        errors.push(format!("`{name}` has width 0"));
                    }
                    if name == clock {
                        errors.push(format!("`{name}` clashes with the clock port name"));
                    }
                    if ctx.signals.insert(name.clone(), (kind, width)).is_some() {
                        errors.push(format!("`{name}` is declared more than once"));
                    }
                }
            }
        }

        // 2. Instructions: every name declared, every destination driven exactly once
        let mut driven: HashSet<&str> = HashSet::new();
        for process in &program.processes {
            for instr in &process.instructions {
                for name in operands(instr) {
                    if !ctx.signals.contains_key(name) {
                        errors.push(format!("`{name}` is used but never declared"));
                    }
                }
                if let Some(rd) = destination(instr) {
                    match ctx.signals.get(rd) {
                        Some((Kind::Wire | Kind::RegNext, _)) => {}
                        Some((kind, _)) => errors.push(format!(
                            "`{rd}` is a {kind:?} and cannot be the destination of an instruction"
                        )),
                        None => {} // already reported as undeclared
                    }
                    if !driven.insert(rd) {
                        errors.push(format!("`{rd}` is assigned by more than one instruction"));
                    }
                }
            }
        }
        if !errors.is_empty() {
            return Err(errors);
        }

        // 3. Widths (warnings only: the Verilog is still valid)
        for process in &program.processes {
            for instr in &process.instructions {
                ctx.check_widths(instr);
            }
        }
        let mut undriven: Vec<&String> = ctx
            .signals
            .iter()
            .filter(|(name, (kind, _))| *kind == Kind::RegNext && !driven.contains(name.as_str()))
            .map(|(name, _)| name)
            .collect();
        undriven.sort(); // HashMap order is random; keep the warnings deterministic
        for name in undriven {
            ctx.warnings
                .push(format!("register input `{name}` is never assigned"));
        }

        // 4. Debug symbols -> aliases (warnings only: a skipped alias just means EQY
        //    has one name less to match)
        let mut used: HashSet<String> = HashSet::new();
        for process in &program.processes {
            for ds in &process.debug_symbols {
                let problem = if !is_simple_identifier(&ds.symbol) {
                    Some("is not a plain Verilog identifier")
                } else if ds.symbol == clock || ctx.signals.contains_key(&ds.symbol) {
                    Some("clashes with a MASM name or the clock")
                } else if !used.insert(ds.symbol.clone()) {
                    Some("is used for more than one signal")
                } else {
                    None
                };
                match problem {
                    Some(p) => ctx.warnings.push(format!(
                        "debug symbol `{}` (for `{}`) {p}; no alias printed",
                        ds.symbol, ds.signal
                    )),
                    None => ctx.aliases.push((ds.symbol.clone(), ds.signal.clone())),
                }
            }
        }

        Ok(ctx)
    }

    pub fn width(&self, name: &str) -> u32 {
        match self.signals.get(name) {
            Some((_, width)) => *width,
            None => panic!("`{name}` is not declared (Context::new should have caught this)"),
        }
    }

    fn check_widths(&mut self, instr: &Instruction) {
        let w = |n: &String| self.width(n);
        // Each arm's guard is the condition under which the instruction is wrong.
        let problem: Option<String> = match instr {
            Instruction::Mov { rd, rs } if w(rd) != w(rs) => {
                Some("MOV operands differ in width".to_string())
            }
            Instruction::Binary { op, rd, rs1, rs2 } => {
                let rule = width_rule(*op);
                let ok = match rule {
                    WidthRule::AllEqual => w(rd) == w(rs1) && w(rd) == w(rs2),
                    WidthRule::Compare => w(rd) == 1 && w(rs1) == w(rs2),
                    WidthRule::Shift => true,
                    WidthRule::ShiftArithmetic => w(rd) == w(rs1),
                };
                (!ok).then(|| format!("{} {}", op.mnemonic(), rule.problem()))
            }
            Instruction::Mux {
                rd,
                sel,
                rfalse,
                rtrue,
            } if w(sel) != 1 || w(rd) != w(rfalse) || w(rd) != w(rtrue) => {
                Some("MUX needs a 1-bit select and equal-width data".to_string())
            }
            Instruction::PadZero { rd, rs, width } if w(rd) != *width || w(rs) > *width => {
                Some("PADZERO width does not match its operands".to_string())
            }
            Instruction::Finish { cond, .. } if w(cond) != 1 => {
                Some("FINISH condition is not 1 bit".to_string())
            }
            _ => None,
        };
        if let Some(problem) = problem {
            self.warnings.push(format!("{problem}: {instr:?}"));
        }
    }
}

/// The width rule an ALU operation must follow. They are based on what
/// manticore_writer.cc writes and what the Scala compiler's interpreter
/// (UnconstrainedInterpreter.scala) and width-conversion pass
/// (WidthConversionCore.scala) accept. A broken rule is a warning, not an error:
/// the Verilog is still valid, but the Scala compiler might reject the MASM or
/// read it differently.
#[derive(Clone, Copy, Debug, PartialEq)]
enum WidthRule {
    /// ADD, SUB, MUL, AND, OR, XOR: `rd`, `rs1` and `rs2` all the same width.
    /// The writer extends or slices both operands to the result width first.
    AllEqual,
    /// SEQ, SLT, SLTS: a 1-bit `rd`, and `rs1` and `rs2` the same width. The writer
    /// extends both operands to the wider of the two; for SLTS this matters,
    /// because the sign bit is the top bit of each operand.
    Compare,
    /// SLL, SRL: no rule. The result is `rs1` shifted, then cut to the width of
    /// `rd`, and the writer uses this: `sextConvert` takes the sign bit of an N-bit
    /// value with `SRL sign(1 bit), value(N bits), N-1`. The shift amount `rs2` can
    /// have any width; concatenations shift by a 32-bit constant.
    Shift,
    /// SRA: `rd` and `rs1` the same width. The sign bit is the top bit of `rs1`;
    /// the width-conversion pass stops with an error when the widths differ.
    ShiftArithmetic,
}

impl WidthRule {
    /// The warning text, after the mnemonic.
    fn problem(self) -> &'static str {
        match self {
            WidthRule::AllEqual => "operands differ in width",
            WidthRule::Compare => "needs a 1-bit result and equal-width operands",
            WidthRule::Shift => unreachable!("shifts have no width rule"),
            WidthRule::ShiftArithmetic => "result and shifted operand differ in width",
        }
    }
}

fn width_rule(op: BinaryOp) -> WidthRule {
    match op {
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::Mul
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Xor => WidthRule::AllEqual,
        BinaryOp::Seq | BinaryOp::Slt | BinaryOp::Slts => WidthRule::Compare,
        BinaryOp::Sll | BinaryOp::Srl => WidthRule::Shift,
        BinaryOp::Sra => WidthRule::ShiftArithmetic,
    }
}

/// SystemVerilog keywords a debug symbol could plausibly be called (not the full list).
const KEYWORDS: &[&str] = &[
    "always",
    "assign",
    "begin",
    "bit",
    "case",
    "default",
    "else",
    "end",
    "for",
    "if",
    "initial",
    "input",
    "int",
    "integer",
    "logic",
    "module",
    "output",
    "parameter",
    "localparam",
    "reg",
    "wire",
    "byte",
    "final",
    "type",
    "string",
];

/// True for names like `total` or `step_2`; false for `top.sub.x`, `\esc`, `mem[3]` or `reg`.
fn is_simple_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !KEYWORDS.contains(&name)
}

fn operands(instr: &Instruction) -> Vec<&String> {
    match instr {
        Instruction::Mov { rd, rs } | Instruction::PadZero { rd, rs, .. } => vec![rd, rs],
        Instruction::Binary { rd, rs1, rs2, .. } => vec![rd, rs1, rs2],
        Instruction::Mux {
            rd,
            sel,
            rfalse,
            rtrue,
        } => vec![rd, sel, rfalse, rtrue],
        Instruction::Finish { cond, .. } => vec![cond],
    }
}

fn destination(instr: &Instruction) -> Option<&str> {
    match instr {
        Instruction::Mov { rd, .. }
        | Instruction::Binary { rd, .. }
        | Instruction::Mux { rd, .. }
        | Instruction::PadZero { rd, .. } => Some(rd),
        Instruction::Finish { .. } => None,
    }
}

/// `"[7:0] "` for width 8, nothing for width 1.
fn range(width: u32) -> String {
    if width == 1 {
        String::new()
    } else {
        format!("[{}:0] ", width - 1)
    }
}

/// A sized binary literal: `(4, 2)` -> `4'b0010`.
fn literal(width: u32, value: &BigUint) -> String {
    format!("{width}'b{value:0w$b}", w = width as usize)
}

/// `wire [7:0] name`
fn decl<'a>(keyword: &'static str, width: u32, name: &'a str) -> Doc<'a> {
    RcDoc::text(keyword)
        .append(" ")
        .append(range(width))
        .append(RcDoc::text(name))
}

/// `assign rd = expr;`
fn assign<'a>(rd: &'a str, expr: Doc<'a>) -> Doc<'a> {
    RcDoc::text("assign ")
        .append(rd)
        .append(" =")
        .append(RcDoc::line().append(expr).nest(INDENT))
        .append(";")
        .group()
}

/// The Verilog operator for a MASM operation.
fn operator(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::And => "&",
        BinaryOp::Or => "|",
        BinaryOp::Xor => "^",
        BinaryOp::Sll => "<<",
        BinaryOp::Srl => ">>",
        BinaryOp::Sra => ">>>",
        BinaryOp::Seq => "==",
        BinaryOp::Slt | BinaryOp::Slts => "<",
    }
}

/// `lhs op rhs`
fn binary<'a>(lhs: Doc<'a>, op: &'static str, rhs: Doc<'a>) -> Doc<'a> {
    lhs.append(" ")
        .append(op)
        .append(RcDoc::line())
        .append(rhs)
        .group()
}

/// `$signed(name)`
fn signed(name: &str) -> Doc<'_> {
    RcDoc::text("$signed(").append(name).append(")")
}

/// The right-hand side of `assign rd = ...;` for an ALU operation.
///
/// Verilog sizes an expression to the widest of its operands *and* the left-hand
/// side of the `assign`, and extends unsigned values with zeros. That is the MASM
/// meaning for everything except the two signed operations:
/// - ADD/SUB/MUL wrap around at the width of `rd`, like the Scala interpreter.
/// - SLL/SRL shift `rs1` and then keep the low bits; if `rd` is wider than `rs1`,
///   the bits shifted out of `rs1` are kept, as in the interpreter.
/// - SEQ/SLT compare two unsigned values and give 1 bit.
///
/// The signed ones need `$signed`, so that the top bit of `rs1` is read as a sign:
/// - SLTS: `$signed(rs1) < $signed(rs2)`, a two's-complement comparison. Both
///   sides must be signed, or Verilog compares them as unsigned.
/// - SRA: `$signed(rs1) >>> rs2` fills with the sign bit (on an unsigned value
///   `>>>` fills with zeros, like `>>`). The shift amount stays unsigned.
fn alu_expr<'a>(op: BinaryOp, rd: &str, rs1: &'a str, rs2: &'a str, ctx: &Context) -> Doc<'a> {
    let plain = |name: &'a str| RcDoc::text(name);
    match op {
        BinaryOp::Slts => binary(signed(rs1), operator(op), signed(rs2)),
        BinaryOp::Sra => {
            let shift = binary(signed(rs1), operator(op), plain(rs2));
            // Normally `rd` is as wide as `rs1` (there is a width warning otherwise).
            // If not, Verilog would sign-extend `rs1` to the width of `rd` first, but
            // the interpreter shifts at the width of `rs1` and then zero-extends or
            // cuts the result. `{...}` makes the shift happen at the width of `rs1`
            // and is unsigned, which gives exactly that.
            if ctx.width(rs1) == ctx.width(rd) {
                shift
            } else {
                RcDoc::text("{").append(shift).append("}")
            }
        }
        _ => binary(plain(rs1), operator(op), plain(rs2)),
    }
}

/// `{{N{1'b0}}, rs}`
fn zero_extend<'a>(rs: &'a str, from: u32, to: u32) -> Doc<'a> {
    use std::cmp::Ordering::*;
    match to.cmp(&from) {
        Equal => RcDoc::text(rs),
        Greater => RcDoc::text(format!("{{{{{}{{1'b0}}}}, ", to - from))
            .append(rs)
            .append("}"),
        // Only reachable after a PADZERO width warning: truncate to the low bits.
        Less => RcDoc::text(rs).append(format!("[{}:0]", to - 1)),
    }
}

/// A `// title` line followed by the items, or nothing if there are no items.
fn section<'a>(title: &str, items: Vec<Doc<'a>>) -> Option<Doc<'a>> {
    if items.is_empty() {
        return None;
    }
    Some(
        RcDoc::text(format!("// {title}"))
            .append(RcDoc::hardline())
            .append(RcDoc::intersperse(items, RcDoc::hardline())),
    )
}

pub fn pp_declaration<'a>(def: &'a Def, _ctx: &Context) -> Doc<'a> {
    match def {
        Def::Wire { name, width } => decl("wire", *width, name).append(";"),
        Def::Const { name, width, value } => decl("localparam", *width, name)
            .append(" = ")
            .append(literal(*width, value))
            .append(";"),
        // A register becomes a `reg` named after its current value and a `wire`
        // for its next value; the register's own name only survives as a comment.
        Def::Reg {
            name,
            width,
            current,
            init,
            next,
        } => {
            let reg = decl("reg", *width, current);
            let reg = match init {
                Some(value) => reg.append(" = ").append(literal(*width, value)),
                None => reg,
            };
            reg.append(format!("; // {name}: current value"))
                .append(RcDoc::hardline())
                .append(decl("wire", *width, next))
                .append(format!("; // {name}: next value"))
        }
    }
}

pub fn pp_instruction<'a>(instr: &'a Instruction, ctx: &Context) -> Doc<'a> {
    match instr {
        Instruction::Mov { rd, rs } => assign(rd, RcDoc::text(rs.as_str())),
        Instruction::Binary { op, rd, rs1, rs2 } => assign(rd, alu_expr(*op, rd, rs1, rs2, ctx)),
        // MASM lists the false value first, Verilog's `?:` the true value.
        Instruction::Mux {
            rd,
            sel,
            rfalse,
            rtrue,
        } => assign(
            rd,
            RcDoc::text(sel.as_str())
                .append(" ?")
                .append(RcDoc::line())
                .append(rtrue.as_str())
                .append(" :")
                .append(RcDoc::line())
                .append(rfalse.as_str())
                .group(),
        ),
        Instruction::PadZero { rd, rs, width } => {
            assign(rd, zero_extend(rs, ctx.width(rs), *width))
        }
        Instruction::Finish { order, cond } => RcDoc::text("always @(*) if (")
            .append(cond.as_str())
            .append(format!(") $finish; // ({order}) FINISH")),
    }
}

/// The `always @(posedge clock)` block, or nothing if there are no registers.
pub fn pp_register_updates<'a>(regs: Vec<&'a Def>, ctx: &Context) -> Option<Doc<'a>> {
    let updates: Vec<Doc<'a>> = regs
        .into_iter()
        .filter_map(|def| match def {
            Def::Reg { current, next, .. } => Some(
                RcDoc::text(current.as_str())
                    .append(" <= ")
                    .append(next.as_str())
                    .append(";"),
            ),
            _ => None,
        })
        .collect();
    if updates.is_empty() {
        return None;
    }
    Some(
        RcDoc::text(format!("always @(posedge {}) begin", ctx.clock))
            .append(
                RcDoc::hardline()
                    .append(RcDoc::intersperse(updates, RcDoc::hardline()))
                    .nest(INDENT),
            )
            .append(RcDoc::hardline())
            .append("end"),
    )
}

/// All processes are printed into one module.
pub fn pp_program<'a>(program: &'a Program, ctx: &'a Context) -> Doc<'a> {
    let defs: Vec<&Def> = program.processes.iter().flat_map(|p| &p.defs).collect();
    let instrs: Vec<&Instruction> = program
        .processes
        .iter()
        .flat_map(|p| &p.instructions)
        .collect();

    let declarations_of = |pick: fn(&Def) -> bool| -> Vec<Doc<'a>> {
        defs.iter()
            .filter(|d| pick(d))
            .map(|d| pp_declaration(d, ctx))
            .collect()
    };
    let (finishes, logic): (Vec<&Instruction>, Vec<&Instruction>) = instrs
        .into_iter()
        .partition(|i| matches!(i, Instruction::Finish { .. }));
    let regs: Vec<&Def> = defs
        .iter()
        .copied()
        .filter(|d| matches!(d, Def::Reg { .. }))
        .collect();

    let sections: Vec<Doc<'a>> = [
        section("wires", declarations_of(|d| matches!(d, Def::Wire { .. }))),
        section(
            "registers",
            declarations_of(|d| matches!(d, Def::Reg { .. })),
        ),
        section(
            "constants",
            declarations_of(|d| matches!(d, Def::Const { .. })),
        ),
        section(
            "debug symbols (original names, kept for EQY)",
            ctx.aliases
                .iter()
                .map(|(symbol, signal)| {
                    // `keep` stops Yosys from removing the alias, so EQY can match it by name.
                    RcDoc::text("(* keep *) ")
                        .append(decl("wire", ctx.width(signal), symbol))
                        .append(" = ")
                        .append(signal.as_str())
                        .append(";")
                })
                .collect(),
        ),
        section(
            "instructions",
            logic.into_iter().map(|i| pp_instruction(i, ctx)).collect(),
        ),
        section(
            "register updates",
            pp_register_updates(regs, ctx).into_iter().collect(),
        ),
        section(
            "system calls",
            if finishes.is_empty() {
                Vec::new()
            } else {
                // $finish is for simulation only. Yosys (and so EQY) defines SYNTHESIS
                // and skips this part, exactly like the guard in the original design.
                std::iter::once(RcDoc::text("`ifndef SYNTHESIS"))
                    .chain(finishes.into_iter().map(|i| pp_instruction(i, ctx)))
                    .chain(std::iter::once(RcDoc::text("`endif")))
                    .collect()
            },
        ),
    ]
    .into_iter()
    .flatten()
    .collect();

    let source = ctx.source.as_deref().unwrap_or("MASM");
    let process_names: Vec<&str> = program.processes.iter().map(|p| p.name.as_str()).collect();

    RcDoc::text(format!(
        "// Generated by masm_parser from {source} (.proc {})",
        process_names.join(", ")
    ))
    .append(RcDoc::hardline())
    .append(format!("module {} (", ctx.top))
    .append(
        RcDoc::hardline()
            .append(format!("input wire {}", ctx.clock))
            .nest(INDENT),
    )
    .append(RcDoc::hardline())
    .append(");")
    .append(
        RcDoc::hardline()
            .append(RcDoc::intersperse(
                sections,
                RcDoc::hardline().append(RcDoc::hardline()),
            ))
            .nest(INDENT),
    )
    .append(RcDoc::hardline())
    .append("endmodule")
    .append(RcDoc::hardline())
}

pub fn render(program: &Program, ctx: &Context) -> String {
    let text = pp_program(program, ctx).pretty(LINE_WIDTH).to_string();
    // The blank lines between sections come out indented; strip that trailing whitespace.
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
