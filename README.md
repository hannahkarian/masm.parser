# masm_parser

A Rust proof of concept that reads **Manticore netlist assembly (MASM)**, stores it
in Rust data structures, and writes it back out as **Verilog**. The generated Verilog
is then compared with the original design using **Logic Equivalence Checking (EQY)**
to show that nothing was lost on the way.

Internship project at QBayLogic (Helios / Manticore), Saxion University of Applied Sciences.
Based on [ManticoreRTL](https://github.com/ManticoreRTL) and the paper
*Manticore: Hardware-Accelerated RTL Simulation with Static Bulk-Synchronous Parallelism*.

## The roundtrip

```
stepper.sv ──(Manticore Yosys frontend)──► stepper.masm
                                              │
                                              │  masm.lalrpop   parse text → AST
                                              ▼
                                         ast.rs types (Program, Def, Instruction)
                                              │
                                              │  verilog.rs     check + print
                                              ▼
                                         generated.sv
                                              │
stepper.sv ◄────────── EQY (stepper.eqy) ─────┘   are they equivalent?
```

## Running it

**1. Verilog → MASM** with the Manticore Yosys fork (`v2masmyosys`, built from
[manticore-frontend](https://github.com/ManticoreRTL/manticore-frontend)). The passes run
in this order: `read_verilog -sv -masm`, `hierarchy`, `proc`, the `opt`/`memory_*`
preparation passes, `flatten`, the `manticore_*` lowering passes, and finally
`manticore_writer`, which writes the `.masm` file.

**2. MASM → Verilog** with this project:

```sh
cargo run -- stepper.masm      # writes generated.sv in the current directory
```

Errors (undeclared names, a signal assigned twice, ...) stop the program; width
mismatches are printed as warnings and generation continues.

**3. Equivalence check**:

```sh
eqy stepper.eqy                # [gold] = stepper.sv, [gate] = generated.sv
```

## Files

| File | What it does |
|------|--------------|
| `src/main.rs` | Entry point: read the file, parse, check, write `generated.sv`. |
| `src/masm.lalrpop` | The grammar. LALRPOP turns it into a Rust parser at build time. |
| `src/ast.rs` | The data types a parsed program is stored in, plus `build_program`. |
| `src/verilog.rs` | `Context` (checks + width table) and the `pp_*` printers that produce Verilog. |
| `build.rs` | Runs LALRPOP before compiling, so the parser is always up to date. |
| `stepper.sv` | Small example design (the reference / "gold" design). |
| `stepper.masm` | Output of the Manticore frontend for `stepper.sv`. |
| `generated.sv` | Output of this tool for `stepper.masm` (the "gate" design). |
| `stepper.eqy` | EQY configuration comparing the two Verilog files. |

Every source file is commented line by line.

## Supported MASM subset

| MASM | Verilog |
|------|---------|
| `.wire w 8` | `wire [7:0] w;` |
| `.const c 4 0b1010` | `localparam [3:0] c = 4'b1010;` |
| `.reg r 8 .input cur 0b0 .output nxt` | `reg [7:0] cur = 8'b0;` + `wire [7:0] nxt;` + `cur <= nxt;` on the clock edge |
| `MOV rd, rs` | `assign rd = rs;` |
| `ADD rd, a, b` | `assign rd = a + b;` |
| `SEQ rd, a, b` | `assign rd = a == b;` |
| `MUX rd, sel, f, t` | `assign rd = sel ? t : f;` (note: MASM lists the *false* value first) |
| `PADZERO rd, rs, 8` | `assign rd = {{N{1'b0}}, rs};` |
| `(0) FINISH c` | `always @(*) if (c) $finish;` |
| `@NAME [ key = "value" ]` | parsed, then ignored |

**Register naming.** A `.reg` has three names. `.input` is the flip-flop's Q (the
value stored now, an *input* to the logic) and `.output` is its D (the value for
the next clock edge, an *output* of the logic). This follows `manticore_writer.cc`.

## Duplicate names and duplicate assignments

`Context::new` in `verilog.rs` checks two different kinds of duplicates:

| Problem | Example | How it is detected |
|---------|---------|--------------------|
| A name declared twice | `.wire ys_w3 4` appears two times | `HashMap::insert` returns `Some(old value)` if the name was already in the `signals` table |
| A signal assigned twice | two instructions both write `ys_w4` | `HashSet::insert` returns `false` if the name was already in the `driven` set |

The second case matters because two `assign`s to the same wire in Verilog means two
drivers, which is an error. A `HashMap` is used for declarations because it also has
to store each name's kind and width; the `HashSet` only needs to remember names.

**What the Manticore frontend does.** `manticore_writer.cc` has no duplicate check;
it avoids duplicates by construction:

- **Unique names.** `NameBuilder::next()` builds names as `ys_` + prefix + counter and
  increments the counter every time. Wires (`w`) and registers (`r`, `i`, `o`) share
  one counter, which is why `stepper.masm` has `ys_r0, ys_i1, ys_o2, ys_w3, ...`.
  Constants (`c`) have their own counter, but the different prefix keeps
  `ys_c3` and `ys_w3` apart.
- **Each object declared once.** Before writing a `.wire`, `.reg` or `.const`, the
  writer looks it up in a `dict` (`wires`, `states`, `constants`). If it already
  exists, the existing name is reused. Constants are looked up by value, so the
  constant `1` gets a single `.const` that all instructions share.
- **One driver per signal.** Yosys runs `check -assert` earlier in the pipeline,
  which rejects designs where a signal has more than one driver.

So MASM straight from the frontend should never contain duplicates. The checks in
this tool are a safety net for MASM from other sources (hand-written test files,
edited files, or a different version of the writer). The frontend guarantees
uniqueness by construction; this tool verifies it instead of assuming it.

## Worked example: `stepper`

`stepper.sv` counts `step` up by 1 every clock, adds 2 to `total` whenever
`step == 3`, and stops the simulation when `total == 6`. In the MASM the names are
replaced by Yosys-generated ones:

| MASM name | Meaning in `stepper.sv` |
|-----------|-------------------------|
| `ys_i1` / `ys_w3` | `total` (current value) |
| `ys_o2` / `ys_w4` | next value of `total` = `step == 3 ? total + 2 : total` |
| `ys_i15` / `ys_w5` | `step` (current value) |
| `ys_o16` / `ys_w7` | next value of `step` = `step + 1` |
| `ys_w10` | `step == 3` |
| `ys_w11` | `total == 6` |
| `ys_i18` / `ys_w20` | registered "finish" flag; `FINISH` fires when it is 1 |
| `ys_c0`, `ys_c1`, ... | constants 1, 2, 6, 3, 0; most are zero-extended to 4 bits with `PADZERO` |

The finish condition goes through a register (`ys_r17`), so it changes right after a
clock edge. That is why `always @(*) if (ys_w20) $finish;` behaves like the original
`$finish` inside `always @(posedge clock)`.

## Limitations

- Only the instructions in the table above are supported; anything else
  (memories, other ALU ops, `$display`, ...) is a parse error.
- Annotations allow a single `key = "value"` pair and are not kept.
- Constants are limited to 64 bits (`u64`).
- One clock, named `clock`; module name is `Main`.

### `$finish` and EQY

- **Not checked by EQY.** EQY compares the *logic* of the two designs. `$finish` is a
  simulation command, not hardware, so whether the simulation stops at the right
  moment is not covered by the equivalence check. To test that, run both designs in a
  simulator (e.g. Icarus Verilog or Verilator) and compare when they stop.
- **`` `ifndef SYNTHESIS `` guard.** In `stepper.sv`, the `$finish` is wrapped in
  `` `ifndef SYNTHESIS `` / `` `endif ``. Yosys's `read_verilog` defines `SYNTHESIS` by
  default, so that code is left out of the gold design. `verilog.rs` currently prints
  `always @(*) if (c) $finish;` **without** this guard, so the gate design still
  contains it. Depending on the Yosys version this can give warnings or errors. Until the
  printer emits the guard itself, it has to be added to `generated.sv` by hand, and a
  hand-edited file no longer matches what `cargo run` produces.
- **Sanity check.** `Main` has no output ports, only `clock`. It is worth confirming that
  EQY really compares the internal signals: change one line of `generated.sv` on purpose
  (for example `+` to `-`) and check that EQY then reports the designs as *not* equivalent.