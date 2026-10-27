use std::collections::{HashMap, HashSet}; // HashMap: key -> value table. HashSet: list of unique values.

use pretty::RcDoc; // RcDoc = a "document": text plus info about where lines may break

use crate::ast::{Def, Instruction, Program}; // the AST types from ast.rs

pub type Doc<'a> = RcDoc<'a, ()>; // short name: Doc<'a> means RcDoc<'a, ()>. 'a = may borrow text from the AST

const INDENT: isize = 4; // 4 spaces per indentation level
const LINE_WIDTH: usize = 100; // try to keep lines at most 100 characters

#[derive(Clone, Copy, PartialEq, Debug)] // allow copying, comparing with ==, and printing with {:?}
enum Kind { // what a name was declared as
    Wire, // a .wire
    Const, // a .const
    RegCurrent, // the `.input` name of a .reg = value stored in the register now
    RegNext, // the `.output` name of a .reg = value it gets on the next clock edge
}

pub struct Context { // extra information the printers need
    pub top: String, // module name, "Main"
    pub clock: String, // clock port name, "clock"
    pub source: Option<String>, // input file name for the header comment, or None
    pub warnings: Vec<String>, // problems that don't stop generation
    signals: HashMap<String, (Kind, u32)>, // table: signal name -> (kind, width in bits)
}

impl Context { // functions that belong to Context
    // check the program and build the table; returns Ok(Context) or Err(list of errors)
    pub fn new(program: &Program, top: &str, clock: &str) -> Result<Context, Vec<String>> {
        let mut ctx = Context { // create an empty Context
            top: top.to_string(), // copy "Main" into an owned String
            clock: clock.to_string(), // copy "clock" into an owned String
            source: None, // no file name yet (main.rs sets it)
            warnings: Vec::new(), // no warnings yet
            signals: HashMap::new(), // empty signal table
        };
        let mut errors = Vec::new(); // list where we collect error messages

        // 1. declarations
        for process in &program.processes { // go through every .proc
            for def in &process.defs { // go through every .wire / .const / .reg in it
                let names: Vec<(&String, Kind, u32)> = match def { // list of (name, kind, width) this def declares
                    Def::Wire { name, width } => vec![(name, Kind::Wire, *width)], // a wire declares 1 name
                    Def::Const { name, width, .. } => vec![(name, Kind::Const, *width)], // a const declares 1 name (`..` skips value)
                    Def::Reg { // a register...
                        width, // ...take its width
                        current, // ...its current-value name (.input)
                        next, // ...its next-value name (.output)
                        .. // ...and ignore name and init
                    } => vec![ // ...declares 2 names:
                        (current, Kind::RegCurrent, *width), // the current value
                        (next, Kind::RegNext, *width), // the next value
                    ],
                };
                for (name, kind, width) in names { // check each declared name
                    if width == 0 { // a 0-bit signal is invalid
                        errors.push(format!("`{name}` has width 0")); // add an error message
                    }
                    if name == clock { // a signal called "clock" would clash with the port
                        errors.push(format!("`{name}` clashes with the clock port name")); // add an error message
                    }
                    // insert into the table; .is_some() is true if the name was already there
                    if ctx.signals.insert(name.clone(), (kind, width)).is_some() {
                        errors.push(format!("`{name}` is declared more than once")); // add an error message
                    }
                }
            }
        }

        // 2. instructions: every name declared, every destination driven once
        let mut driven: HashSet<&str> = HashSet::new(); // names that already have an instruction writing them
        for process in &program.processes { // go through every .proc
            for instr in &process.instructions { // go through every instruction in it
                for name in operands(instr) { // every name the instruction uses
                    if !ctx.signals.contains_key(name) { // not in the table?
                        errors.push(format!("`{name}` is used but never declared")); // add an error message
                    }
                }
                if let Some(rd) = destination(instr) { // if the instruction writes to something (FINISH doesn't)
                    match ctx.signals.get(rd) { // look up what the destination is
                        Some((Kind::Wire | Kind::RegNext, _)) => {} // a wire or a register's next value: allowed
                        Some((kind, _)) => errors.push(format!( // a const or a register's current value: not allowed
                            "`{rd}` is a {kind:?} and cannot be the destination of an instruction"
                        )),
                        None => {} // not declared: already reported above
                    }
                    if !driven.insert(rd) { // insert returns false if rd was already in the set
                        errors.push(format!("`{rd}` is assigned by more than one instruction")); // add an error message
                    }
                }
            }
        }
        if !errors.is_empty() { // if we found any errors...
            return Err(errors); // ...stop here and return them all
        }

        // 3. width checks (warnings only: the Verilog is still valid)
        for process in &program.processes { // go through every .proc
            for instr in &process.instructions { // go through every instruction
                ctx.check_widths(instr); // add a warning if its widths don't fit
            }
        }
        for (name, (kind, _)) in &ctx.signals { // go through the whole signal table
            if *kind == Kind::RegNext && !driven.contains(name.as_str()) { // a register's next value that nobody writes
                ctx.warnings // add to the warnings list...
                    .push(format!("register input `{name}` is never assigned")); // ...this message
            }
        }

        Ok(ctx) // everything is fine: return the Context
    }

    pub fn width(&self, name: &str) -> u32 { // look up the width of a signal
        match self.signals.get(name) { // search the table
            Some((_, width)) => *width, // found: return its width
            None => panic!("`{name}` is not declared (Context::new should have caught this)"), // not found: crash (should never happen)
        }
    }

    fn check_widths(&mut self, instr: &Instruction) { // warn if an instruction's widths don't match
        let w = |n: &String| self.width(n); // small helper: w(name) = width of that name
        let problem = match instr { // each arm only matches when its `if` (the problem) is true
            // MOV: source and destination must be the same width
            Instruction::Mov { rd, rs } if w(rd) != w(rs) => Some("MOV operands differ in width"),
            // ADD: all three must be the same width
            Instruction::Add { rd, rs1, rs2 } if w(rd) != w(rs1) || w(rd) != w(rs2) => {
                Some("ADD operands differ in width") // the warning text
            }
            // SEQ: result must be 1 bit, the two inputs the same width
            Instruction::Seq { rd, rs1, rs2 } if w(rd) != 1 || w(rs1) != w(rs2) => {
                Some("SEQ needs a 1-bit result and equal-width operands") // the warning text
            }
            Instruction::Mux { // MUX: take all four operands
                rd,
                sel,
                rfalse,
                rtrue,
            } if w(sel) != 1 || w(rd) != w(rfalse) || w(rd) != w(rtrue) => { // select must be 1 bit, data the same width
                Some("MUX needs a 1-bit select and equal-width data") // the warning text
            }
            // PADZERO: result must be `width` bits, source not wider than that
            Instruction::PadZero { rd, rs, width } if w(rd) != *width || w(rs) > *width => {
                Some("PADZERO width does not match its operands") // the warning text
            }
            Instruction::Finish { cond, .. } if w(cond) != 1 => { // FINISH: condition must be 1 bit
                Some("FINISH condition is not 1 bit") // the warning text
            }
            _ => None, // any other case: no problem
        };
        if let Some(problem) = problem { // if a problem was found...
            self.warnings.push(format!("{problem}: {instr:?}")); // ...add it plus the instruction to the warnings
        }
    }
}

fn operands(instr: &Instruction) -> Vec<&String> { // list every name an instruction uses
    match instr { // check which instruction it is
        Instruction::Mov { rd, rs } | Instruction::PadZero { rd, rs, .. } => vec![rd, rs], // MOV and PADZERO: 2 names
        Instruction::Add { rd, rs1, rs2 } | Instruction::Seq { rd, rs1, rs2 } => vec![rd, rs1, rs2], // ADD and SEQ: 3 names
        Instruction::Mux { // MUX...
            rd,
            sel,
            rfalse,
            rtrue,
        } => vec![rd, sel, rfalse, rtrue], // ...4 names
        Instruction::Finish { cond, .. } => vec![cond], // FINISH: 1 name (`..` skips order)
    }
}

fn destination(instr: &Instruction) -> Option<&str> { // the name an instruction writes to, if any
    match instr { // check which instruction it is
        Instruction::Mov { rd, .. } // MOV,
        | Instruction::Add { rd, .. } // ADD,
        | Instruction::Seq { rd, .. } // SEQ,
        | Instruction::Mux { rd, .. } // MUX
        | Instruction::PadZero { rd, .. } => Some(rd), // and PADZERO all write to rd
        Instruction::Finish { .. } => None, // FINISH writes nothing
    }
}

fn range(width: u32) -> String { // the bit range text for a width
    if width == 1 { // 1 bit needs no range
        String::new() // so return an empty string
    } else { // otherwise
        format!("[{}:0] ", width - 1) // e.g. width 8 -> "[7:0] "
    }
}

fn literal(width: u32, value: u64) -> String { // a sized binary number, e.g. (4, 2) -> "4'b0010"
    format!("{width}'b{value:0w$b}", w = width as usize) // {value:0w$b} = value in binary, zero-padded to w digits
}

fn decl<'a>(keyword: &'static str, width: u32, name: &'a str) -> Doc<'a> { // e.g. "wire [7:0] name"
    RcDoc::text(keyword) // "wire" / "reg" / "localparam"
        .append(" ") // a space
        .append(range(width)) // "[7:0] " or nothing
        .append(RcDoc::text(name)) // the signal name
}

fn assign<'a>(rd: &'a str, expr: Doc<'a>) -> Doc<'a> { // "assign rd = expr;"
    RcDoc::text("assign ") // "assign "
        .append(rd) // the destination name
        .append(" =") // " ="
        .append(RcDoc::line().append(expr).nest(INDENT)) // a space (or newline + indent if too long), then the expression
        .append(";") // ";"
        .group() // try to keep it all on one line
}

fn binary<'a>(lhs: &'a str, op: &'static str, rhs: &'a str) -> Doc<'a> { // "lhs op rhs", e.g. "a + b"
    RcDoc::text(lhs) // left operand
        .append(" ") // a space
        .append(op) // the operator, "+" or "=="
        .append(RcDoc::line()) // a space (or a newline if too long)
        .append(rhs) // right operand
        .group() // try to keep it on one line
}

fn zero_extend<'a>(rs: &'a str, from: u32, to: u32) -> Doc<'a> { // add zeros in front of rs to make it `to` bits wide
    use std::cmp::Ordering::*; // so we can write Equal/Greater/Less instead of Ordering::Equal etc.
    match to.cmp(&from) { // compare the target width with the current width
        Equal => RcDoc::text(rs), // same width: just rs
        // wider: "{{N{1'b0}}, rs}" ({{ and }} in format! print a single { and })
        Greater => RcDoc::text(format!("{{{{{}{{1'b0}}}}, ", to - from))
            .append(rs) // then rs
            .append("}"), // then the closing }
        Less => RcDoc::text(rs).append(format!("[{}:0]", to - 1)), // narrower (shouldn't happen): keep only the low bits
    }
}

fn section<'a>(title: &str, items: Vec<Doc<'a>>) -> Option<Doc<'a>> { // "// title" followed by items, one per line
    if items.is_empty() { // nothing to print?
        return None; // then leave the whole section out
    }
    Some( // otherwise return:
        RcDoc::text(format!("// {title}")) // the title comment
            .append(RcDoc::hardline()) // a newline
            .append(RcDoc::intersperse(items, RcDoc::hardline())), // the items with a newline between each
    )
}

pub fn pp_declaration<'a>(def: &'a Def, _ctx: &Context) -> Doc<'a> { // print one .wire/.const/.reg (_ctx unused)
    match def { // check which kind of definition
        Def::Wire { name, width } => decl("wire", *width, name).append(";"), // "wire [3:0] ys_w3;"
        Def::Const { name, width, value } => decl("localparam", *width, name) // "localparam [1:0] ys_c1"
            .append(" = ") // " = "
            .append(literal(*width, *value)) // "2'b10"
            .append(";"), // ";"
        Def::Reg { // a register: take all its fields
            name,
            width,
            current,
            init,
            next,
        } => {
            let reg = decl("reg", *width, current); // "reg [3:0] ys_i1" (named after the current value)
            let reg = match init { // does it have an initial value?
                Some(value) => reg.append(" = ").append(literal(*width, *value)), // yes: add " = 4'b0000"
                None => reg, // no: keep it as it is
            };
            reg.append(format!("; // {name}: current value")) // "; // ys_r0: current value"
                .append(RcDoc::hardline()) // newline
                .append(decl("wire", *width, next)) // "wire [3:0] ys_o2"
                .append(format!("; // {name}: next value")) // "; // ys_r0: next value"
        }
    }
}

pub fn pp_instruction<'a>(instr: &'a Instruction, ctx: &Context) -> Doc<'a> { // print one instruction
    match instr { // check which instruction it is
        Instruction::Mov { rd, rs } => assign(rd, RcDoc::text(rs.as_str())), // "assign rd = rs;"
        Instruction::Add { rd, rs1, rs2 } => assign(rd, binary(rs1, "+", rs2)), // "assign rd = rs1 + rs2;"
        Instruction::Seq { rd, rs1, rs2 } => assign(rd, binary(rs1, "==", rs2)), // "assign rd = rs1 == rs2;"
        Instruction::Mux { // MUX: take all four operands
            rd,
            sel,
            rfalse,
            rtrue,
        } => assign( // "assign rd = ..."
            rd,
            RcDoc::text(sel.as_str()) // "sel"
                .append(" ?") // " ?"
                .append(RcDoc::line()) // space (or newline)
                .append(rtrue.as_str()) // the TRUE value comes first in Verilog
                .append(" :") // " :"
                .append(RcDoc::line()) // space (or newline)
                .append(rfalse.as_str()) // then the FALSE value
                .group(), // try to keep it on one line
        ),
        Instruction::PadZero { rd, rs, width } => { // PADZERO
            assign(rd, zero_extend(rs, ctx.width(rs), *width)) // "assign rd = {{N{1'b0}}, rs};" (needs rs's width from ctx)
        }
        Instruction::Finish { order, cond } => RcDoc::text("always @(*) if (") // "always @(*) if ("
            .append(cond.as_str()) // the condition name
            .append(format!(") $finish; // ({order}) FINISH")), // ") $finish; // (0) FINISH"
    }
}

pub fn pp_register_updates<'a>(regs: Vec<&'a Def>, ctx: &Context) -> Option<Doc<'a>> { // the clocked block that updates registers
    let updates: Vec<Doc<'a>> = regs // start from the list of registers
        .into_iter() // go through them one by one
        .filter_map(|def| match def { // turn each into a line, skipping non-registers
            Def::Reg { current, next, .. } => Some( // a register becomes:
                RcDoc::text(current.as_str()) // "ys_i1"
                    .append(" <= ") // " <= " (updated on the clock edge)
                    .append(next.as_str()) // "ys_o2"
                    .append(";"), // ";"
            ),
            _ => None, // anything else: skip
        })
        .collect(); // put the lines in a Vec
    if updates.is_empty() { // no registers?
        return None; // then no block at all
    }
    Some( // otherwise return:
        RcDoc::text(format!("always @(posedge {}) begin", ctx.clock)) // "always @(posedge clock) begin"
            .append(
                RcDoc::hardline() // newline
                    .append(RcDoc::intersperse(updates, RcDoc::hardline())) // the update lines, one per line
                    .nest(INDENT), // indented by 4
            )
            .append(RcDoc::hardline()) // newline
            .append("end"), // "end"
    )
}

pub fn pp_program<'a>(program: &'a Program, ctx: &'a Context) -> Doc<'a> { // print the whole module
    let defs: Vec<&Def> = program.processes.iter().flat_map(|p| &p.defs).collect(); // all defs of all processes in one list
    let instrs: Vec<&Instruction> = program // all instructions of all processes in one list:
        .processes // the processes
        .iter() // go through them
        .flat_map(|p| &p.instructions) // take each process's instructions and chain them together
        .collect(); // put them in a Vec

    // helper: print only the defs for which `pick` returns true
    let declarations_of = |pick: fn(&Def) -> bool| -> Vec<Doc<'a>> {
        defs.iter() // go through all defs
            .filter(|d| pick(d)) // keep the ones `pick` accepts
            .map(|d| pp_declaration(d, ctx)) // print each one
            .collect() // put the results in a Vec
    };
    // split the instructions: FINISH ones into `finishes`, all others into `logic`
    let (finishes, logic): (Vec<&Instruction>, Vec<&Instruction>) = instrs
        .into_iter() // go through the instructions
        .partition(|i| matches!(i, Instruction::Finish { .. })); // true -> finishes, false -> logic
    let regs: Vec<&Def> = defs // only the register defs:
        .iter() // go through all defs
        .copied() // turn &&Def into &Def
        .filter(|d| matches!(d, Def::Reg { .. })) // keep only registers
        .collect(); // put them in a Vec

    let sections: Vec<Doc<'a>> = [ // build all sections in file order
        section("wires", declarations_of(|d| matches!(d, Def::Wire { .. }))), // "// wires" + all wires
        section(
            "registers", // "// registers"
            declarations_of(|d| matches!(d, Def::Reg { .. })), // + all registers
        ),
        section(
            "constants", // "// constants"
            declarations_of(|d| matches!(d, Def::Const { .. })), // + all constants
        ),
        section(
            "instructions", // "// instructions"
            logic.into_iter().map(|i| pp_instruction(i, ctx)).collect(), // + every non-FINISH instruction
        ),
        section(
            "register updates", // "// register updates"
            pp_register_updates(regs, ctx).into_iter().collect(), // + the always block (Option -> Vec of 0 or 1)
        ),
        section(
            "system calls", // "// system calls"
            finishes // + every FINISH
                .into_iter() // go through them
                .map(|i| pp_instruction(i, ctx)) // print each one
                .collect(), // put them in a Vec
        ),
    ]
    .into_iter() // go through the sections
    .flatten() // drop the empty ones (None), unwrap the rest (Some)
    .collect(); // put them in a Vec

    let source = ctx.source.as_deref().unwrap_or("MASM"); // the input file name, or "MASM" if not set
    let process_names: Vec<&str> = program.processes.iter().map(|p| p.name.as_str()).collect(); // names of all processes, e.g. ["main"]

    RcDoc::text(format!( // first line:
        "// Generated by masm_parser from {source} (.proc {})", // "// Generated by masm_parser from stepper.masm (.proc main)"
        process_names.join(", ") // process names separated by ", "
    ))
    .append(RcDoc::hardline()) // newline
    .append(format!("module {} (", ctx.top)) // "module Main ("
    .append(
        RcDoc::hardline() // newline
            .append(format!("input wire {}", ctx.clock)) // "input wire clock"
            .nest(INDENT), // indented by 4
    )
    .append(RcDoc::hardline()) // newline
    .append(");") // ");"
    .append(
        RcDoc::hardline() // newline
            .append(RcDoc::intersperse( // all sections...
                sections,
                RcDoc::hardline().append(RcDoc::hardline()), // ...with an empty line between them
            ))
            .nest(INDENT), // indented by 4
    )
    .append(RcDoc::hardline()) // newline
    .append("endmodule") // "endmodule"
    .append(RcDoc::hardline()) // final newline
}

pub fn render(program: &Program, ctx: &Context) -> String { // turn the whole program into a String
    let text = pp_program(program, ctx).pretty(LINE_WIDTH).to_string(); // build the document, lay it out at 100 chars, make text
    text.lines() // split into lines
        .map(str::trim_end) // remove trailing spaces from each line
        .collect::<Vec<_>>() // collect the lines into a Vec
        .join("\n") // join them back with newlines
        + "\n" // add a final newline
}