# masm_parser

A parser for **Manticore assembly (MASM)**, written in Rust with [LALRPOP](https://lalrpop.github.io/lalrpop/).

It reads the `main.masm` file produced by the [Manticore](https://github.com/ManticoreRTL) frontend and turns it into Rust data structures (an abstract syntax tree). This is the first step towards converting MASM back to Verilog and checking that the result is equivalent to the original design.

This repository is part of my internship project.

## Background

Manticore is a hardware-accelerated RTL simulator. Its frontend is a modified Yosys (`v2masmyosys`) that compiles a Verilog design into MASM, a low-level assembly language with wires, registers, constants and simple instructions:

```
Verilog (.sv)  ──►  v2masmyosys  ──►  main.masm  ──►  masm_parser  ──►  Rust AST
```

The grammar follows the original Scala parser (`UnconstrainedAssemblyParser.scala`) and the output format of `manticore_writer.cc` from the Manticore repositories.

## Supported syntax

| Kind | Syntax | Example |
|---|---|---|
| Program / process | `.prog:` / `.proc name:` | `.proc main:` |
| Wire | `.wire name width` | `.wire ys_w3 1` |
| Constant | `.const name width value` | `.const ys_c0 1 0b1` |
| Register | `.reg name width .input cur [init] .output next` | `.reg ys_r0 1 .input ys_i1 0b0 .output ys_o2` |
| Move | `MOV rd, rs;` | `MOV ys_w3, ys_i1;` |
| Finish | `(order) FINISH cond;` | `(0) FINISH ys_w3;` |
| Annotation | `@NAME [ key = "value" ]` | `@SOURCEINFO [ file = "finish.sv:4.9-4.16" ]` |
| Comment | `// ...` | `// wires` |

Numbers can be decimal (`3`) or binary (`0b101`).

## Requirements

- Rust and Cargo (a recent stable version; install with [rustup](https://rustup.rs))
- `lalrpop` 0.23.1 (downloaded automatically by Cargo)
- To generate your own `.masm` files: the Manticore frontend `v2masmyosys`

## Build and run

```bash
git clone <this-repo-url>
cd masm_parser
cargo run -- finish.masm
```

The `--` separates Cargo's arguments from the program's arguments. On success, the parsed program is printed:

```
Program {
    processes: [
        Process {
            name: "main",
            defs: [
                Wire { name: "ys_w3", width: 1 },
                Reg { name: "ys_r0", width: 1, current: "ys_i1", init: Some(0), next: "ys_o2" },
                Const { name: "ys_c0", width: 1, value: 1 },
            ],
            instructions: [
                Mov { rd: "ys_w3", rs: "ys_i1" },
                Mov { rd: "ys_o2", rs: "ys_c0" },
                Finish { order: 0, cond: "ys_w3" },
            ],
        },
    ],
}
```

On a syntax error, the parser reports the position and the expected tokens, for example:

```
parse error: Unrecognized token `b` found at 26:27
Expected one of ","
```

## Generating a MASM file from Verilog

The example `finish.masm` was generated from this design:

```verilog
// finish.sv
module Main (input wire clock);
    always @(posedge clock)
        $finish;
endmodule
```

with the Manticore frontend:

```bash
./v2masmyosys -p "read_verilog -sv -masm finish.sv; hierarchy -check; hierarchy -auto-top; proc; opt -purge; wreduce -memx; opt_reduce; opt_clean; check -assert; memory_collect; memory_unpack; memory_dff; manticore_init; flatten; manticore_meminit; opt -purge; manticore_dff; manticore_opt_replicate; manticore_subword; manticore_check; manticore_writer main.masm"
```

A design must contain a `$finish` (or `$display`). Otherwise Yosys removes all logic, because the top module has no outputs.

## Project structure

```
masm_parser/
├── Cargo.toml         # dependencies: lalrpop (build), lalrpop-util (runtime)
├── build.rs           # generates the parser from src/masm.lalrpop at build time
├── finish.masm        # example input
└── src/
    ├── masm.lalrpop   # the grammar: lexer rules and parser rules
    ├── ast.rs         # Rust types for the parsed program + build_program()
    └── main.rs        # reads the file, runs the parser, prints the result
```

How it works:

1. `build.rs` calls `lalrpop::process_src()`, which turns `src/masm.lalrpop` into Rust code during `cargo build`.
2. `main.rs` includes that code with `lalrpop_mod!(masm)` and calls `masm::ProgramParser::new().parse(&text)`.
3. The grammar produces a flat list of items (annotations, process headers, definitions, instructions). `build_program()` in `ast.rs` groups them into processes. This flat design avoids LALR conflicts, because annotations can appear before both definitions and instructions.


## References

- Manticore: <https://github.com/ManticoreRTL>
- M. Emami et al., *Manticore: Hardware-Accelerated RTL Simulation with Static Bulk-Synchronous Parallelism*
- LALRPOP book: <https://lalrpop.github.io/lalrpop/>
- The Rust Programming Language: <https://doc.rust-lang.org/book/>

## Author

Hana Sabzikarian
