#![allow(dead_code)] // some fields are only printed for now

#[derive(Debug)]
pub struct Program {
    pub processes: Vec<Process>,
}

/// One `.proc <name>:` block.
#[derive(Debug)]
pub struct Process {
    pub name: String,
    pub defs: Vec<Def>,
    pub instructions: Vec<Instruction>,
    pub debug_symbols: Vec<DebugSymbol>,
}

/// `@DEBUGSYMBOL [ symbol = "total" ]` written just before `.wire ys_w3 4` means:
/// the MASM signal `ys_w3` is called `total` in the original Verilog.
#[derive(Debug, PartialEq)]
pub struct DebugSymbol {
    pub symbol: String,
    pub signal: String,
}

/// `@SOURCEINFO [ file = "finish.sv:4.9-4.16" ]`
#[derive(Debug)]
pub struct Annotation {
    pub name: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum Def {
    /// `.wire ys_w3 1`
    Wire { name: String, width: u32 },
    /// `.const ys_c0 1 0b1`
    Const {
        name: String,
        width: u32,
        value: u64,
    },
    /// `.reg ys_r0 1 .input ys_i1 0b0 .output ys_o2`
    ///
    /// `current` (`.input`) is the stored value the logic reads, `next` (`.output`)
    /// is the value it takes on the next clock edge.
    Reg {
        name: String,
        width: u32,
        current: String,
        init: Option<u64>,
        next: String,
    },
}

#[derive(Debug)]
pub enum Instruction {
    /// `MOV rd, rs;`
    Mov { rd: String, rs: String },
    /// `ADD rd, rs1, rs2;`
    Add {
        rd: String,
        rs1: String,
        rs2: String,
    },
    /// `SEQ rd, rs1, rs2;` — `rd = (rs1 == rs2)`, a 1-bit result
    Seq {
        rd: String,
        rs1: String,
        rs2: String,
    },
    /// `MUX rd, sel, rfalse, rtrue;` — note the false value comes first
    /// (same order as `instr.MUX` in manticore_writer.cc).
    Mux {
        rd: String,
        sel: String,
        rfalse: String,
        rtrue: String,
    },
    /// `PADZERO rd, rs, width;` — `rs` zero-extended to `width` bits
    PadZero { rd: String, rs: String, width: u32 },
    /// `(order) FINISH cond;`
    Finish { order: u32, cond: String },
}

/// What the grammar produces: a flat list of these, which `build_program` groups
/// into processes.
#[derive(Debug)]
pub enum Item {
    Annotation(Annotation),
    Proc(String),
    Def(Def),
    Instruction(Instruction),
}

impl Def {
    /// The name an annotation in front of this def refers to.
    pub fn signal_name(&self) -> &str {
        match self {
            Def::Wire { name, .. } | Def::Const { name, .. } => name,
            Def::Reg { current, .. } => current,
        }
    }
}

/// Annotations are written *before* the item they describe, so they are collected
/// in `pending` and handed to the next item. Only `@DEBUGSYMBOL` on a def is kept
/// for now; all other annotations are dropped.
pub fn build_program(items: Vec<Item>) -> Result<Program, String> {
    let mut processes: Vec<Process> = Vec::new();
    let mut pending: Vec<Annotation> = Vec::new();
    for item in items {
        if let Item::Annotation(a) = item {
            pending.push(a);
            continue;
        }
        let annotations = std::mem::take(&mut pending);
        if let Item::Proc(name) = item {
            processes.push(Process {
                name,
                defs: Vec::new(),
                instructions: Vec::new(),
                debug_symbols: Vec::new(),
            });
            continue;
        }
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
            Item::Instruction(i) => process.instructions.push(i),
            Item::Proc(_) | Item::Annotation(_) => unreachable!("handled above"),
        }
    }
    Ok(Program { processes })
}
