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

// groups values into a program
pub fn build_program(items: Vec<Item>) -> Program {
    let mut processes: Vec<Process> = Vec::new();
    for item in items {
        match item {
            // if it's ".proc main:", create a new empty Process and push it to the processes vector
            Item::Proc(name) => processes.push(Process {
                name,
                defs: Vec::new(),
                instructions: Vec::new(),
            }),
            // if it's a Def, push it to the defs vector of the last Process in processes
            Item::Def(d) => processes
                .last_mut() // Optional<&mut Process> a mutable reference to the last process, or None if the list is empty
                .expect("def outside .proc")
                .defs
                .push(d), // adds the definition to that process's list
            Item::Instruction(i) => processes
                .last_mut()
                .expect("instruction outside .proc")
                .instructions
                .push(i),
            Item::Annotation(_) => {} // ignored for now
        }
    }
    Program { processes }
}
