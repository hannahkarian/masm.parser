#![allow(dead_code)] // fields are only printed for now
#[derive(Debug)]
pub struct Program {
    pub processes: Vec<Process>,
}

#[derive(Debug)]
//One .proc main: block
pub struct Process {
    pub name: String,                   // main
    pub defs: Vec<Def>,                 // .wire, .reg, .const
    pub instructions: Vec<Instruction>, // MOV, FINISH
    pub debug_symbols: Vec<DebugSymbol>, // original names from @DEBUGSYMBOL annotations
}

// @DEBUGSYMBOL [ symbol = "total" ] written just before `.wire ys_w3 4`
// means: the MASM signal ys_w3 is called `total` in the original Verilog.
#[derive(Debug, PartialEq)]
pub struct DebugSymbol {
    pub symbol: String, // "total" (name in the original design)
    pub signal: String, // "ys_w3" (name in the MASM)
}

#[derive(Debug)]
pub struct Annotation {
    pub name: String, // "SOURCEINFO"
    pub fields: Vec<(String, String)>, // [("file", "/home/.../finish.sv:4.9-4.16")]
                      // (String, String) is a tuple, a fixed size of two elements
                      // each tuple is one key-value pair
}

#[derive(Debug)]
pub enum Def {
    // .wire ys_w3 1
    Wire {
        name: String,
        width: u32,
    },
    // .const ys_c0 1 0b1
    Const {
        name: String,
        width: u32,
        value: u64,
    },
    // .reg ys_r0 1 .input ys_i1 0b0 .output ys_o2
    Reg {
        name: String,
        width: u32,
        current: String,
        init: Option<u64>, // either Some(number) or None
        // init is an Option because a .reg doesn't always have an initial value
        next: String,
    },
}

#[derive(Debug)]
pub enum Instruction {
    // MOV ys_w3, ys_i1;
    Mov { rd: String, rs: String }, // rd is the destination register and rs the source register
    // ADD ys_w6, ys_w3, ys_w5;   rd = rs1 + rs2
    Add { rd: String, rs1: String, rs2: String },
    // SEQ ys_w7, ys_w3, ys_c1;   rd = (rs1 == rs2), a 1-bit result
    Seq { rd: String, rs1: String, rs2: String },
    // MUX ys_w4, ys_w7, ys_w6, ys_w3;   rd = sel ? rtrue : rfalse
    // operand order matches instr.MUX(rd, sel, rfalse, rtrue) in manticore_writer.cc
    Mux { rd: String, sel: String, rfalse: String, rtrue: String },
    // PADZERO ys_w5, ys_c0, 3;   rd = rs zero-extended to `width` bits
    PadZero { rd: String, rs: String, width: u32 },
    // (0) FINISH ys_w3;
    Finish { order: u32, cond: String },
}

// The grammar produces a list of these items, and build_program() groups them into processes
#[derive(Debug)]
pub enum Item {
    Annotation(Annotation),
    Proc(String),
    Def(Def),
    Instruction(Instruction),
}

impl Def {
    // the name an annotation in front of this def refers to
    // (for a .reg that is the current value, the name the logic reads)
    pub fn signal_name(&self) -> &str {
        match self {
            Def::Wire { name, .. } | Def::Const { name, .. } => name,
            Def::Reg { current, .. } => current,
        }
    }
}

// groups values into a program
// Annotations are written *before* the item they describe, so they are collected in
// `pending` and handed to the next item. Only @DEBUGSYMBOL on a def is kept for now.
pub fn build_program(items: Vec<Item>) -> Result<Program, String> {
    let mut processes: Vec<Process> = Vec::new();
    let mut pending: Vec<Annotation> = Vec::new(); // annotations waiting for their item
    for item in items {
        if let Item::Annotation(a) = item {
            pending.push(a); // remember it until the next item
            continue;
        }
        let annotations = std::mem::take(&mut pending); // the annotations that belong to this item
        if let Item::Proc(name) = item {
            // ".proc main:" starts a new empty Process
            processes.push(Process {
                name,
                defs: Vec::new(),
                instructions: Vec::new(),
                debug_symbols: Vec::new(),
            });
            continue;
        }
        // defs and instructions go into the last Process; error if there is none yet
        let process = processes
            .last_mut()
            .ok_or_else(|| "definition or instruction outside .proc".to_string())?;
        match item {
            Item::Def(d) => {
                for a in annotations.iter().filter(|a| a.name == "DEBUGSYMBOL") {
                    for (key, value) in &a.fields {
                        if key == "symbol" {
                            process.debug_symbols.push(DebugSymbol {
                                symbol: value.clone(),
                                signal: d.signal_name().to_string(),
                            });
                        }
                    }
                }
                process.defs.push(d);
            }
            Item::Instruction(i) => process.instructions.push(i), // annotations ignored for now
            Item::Proc(_) | Item::Annotation(_) => unreachable!(), // handled above
        }
    }
    Ok(Program { processes })
}
