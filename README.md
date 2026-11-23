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
cargo run -- stepper.masm               # writes generated.sv in the current directory
cargo run -- stepper.masm out.sv        # or choose the output file
```

Errors (undeclared names, a signal assigned twice, ...) stop the program; width
mismatches are printed as warnings and generation continues.

**3. Equivalence check**:

```sh
eqy -f stepper.eqy             # [gold] = stepper.sv, [gate] = generated.sv
eqy -f alu.eqy                 # [gold] = alu.sv,     [gate] = alu_generated.sv
```

**4. Full check (recommended)** — runs EQY on the real design *and* on broken copies:

```sh
cargo test                     # unit tests
scripts/eqy_check.sh           # EQY on stepper and alu: real designs must PASS, 13 broken copies must FAIL
scripts/sim_check.sh           # Icarus Verilog: same trace and same $finish time
```

## How the EQY check works (and why it used to pass for nothing)

`Main` has no output ports. Yosys' `prep` removes every cell whose result nobody can
see, so in the first version **both designs became empty modules** after `prep`
(`gold.il` contained only the `clock` port), EQY made **0 partitions**, and reported
"Successfully proved designs equivalent". Any `generated.sv`, also a wrong one, passed.

The fix gives EQY something to compare, using the original names that the Manticore
frontend already writes into the MASM:

1. `@DEBUGSYMBOL [ symbol = "total" ]` before `.wire ys_w3 4` says that `ys_w3` is
   `total` in `stepper.sv`. `build_program` attaches it to that def, and the generator
   prints `(* keep *) wire [3:0] total = ys_w3;`.
2. In `stepper.eqy` the gold design marks its own named wires `keep`
   (`setattr -set keep 1 w:* w:$* %d` = all wires except auto-generated `$...` ones).
3. Both sides now have kept wires `step` and `total`. EQY matches them by name and
   makes one partition per signal (`Main.step`, `Main.total`): it proves that the
   next-state logic of each register is the same in both designs.
4. The partitions contain registers, so the `sat` strategy (Yosys `sat -tempinduct`,
   no external solver needed) is used.

**Always check that a PASS means something.** `scripts/eqy_check.sh` checks that
`partition.list` is not empty and runs EQY on broken copies of `generated.sv`
(`+` → `-`, MUX arms swapped, wrong constants, wrong PADZERO, register not updated,
wrong initial value), and of `alu_generated.sv` (each of SUB, MUL, AND, OR, XOR
replaced by another operation, and SUB with its operands swapped). Each must FAIL,
and it does, in the partition you would expect.

## Files

| File | What it does |
|------|--------------|
| `src/main.rs` | Entry point: read the file, parse, check, write `generated.sv`. |
| `src/tests.rs` | Unit tests (`cargo test`). |
| `src/masm.lalrpop` | The grammar. LALRPOP turns it into a Rust parser at build time. |
| `src/ast.rs` | The data types a parsed program is stored in, plus `build_program`. |
| `src/verilog.rs` | `Context` (checks + width table) and the `pp_*` printers that produce Verilog. |
| `.github/workflows/ci.yml` | CI: runs the unit tests and checks that the committed `generated.sv` and `alu_generated.sv` are up to date. |
| `build.rs` | Runs LALRPOP before compiling, so the parser is always up to date. |
| `stepper.sv` | Small example design (the reference / "gold" design). |
| `stepper.masm` | Output of the Manticore frontend for `stepper.sv`. |
| `generated.sv` | Output of this tool for `stepper.masm` (the "gate" design). |
| `stepper.eqy` | EQY configuration comparing the two Verilog files. |
| `alu.sv`, `alu.masm`, `alu_generated.sv`, `alu.eqy` | The same four files for a second design, which tests SUB, MUL, AND, OR and XOR. |
| `scripts/eqy_check.sh` | EQY on each real design, stepper and alu (must PASS), and on broken copies (must FAIL). |
| `scripts/sim_check.sh` | Simulates both designs with `tests/tb_stepper.sv` and compares them, incl. `$finish`. |

EQY and simulation work directories (`stepper/`, `alu/`, `build/`) are not committed.

Every source file is commented line by line.

## Supported MASM subset

| MASM | Verilog |
|------|---------|
| `.wire w 8` | `wire [7:0] w;` |
| `.const c 4 0b1010` | `localparam [3:0] c = 4'b1010;` |
| `.reg r 8 .input cur 0b0 .output nxt` | `reg [7:0] cur = 8'b0;` + `wire [7:0] nxt;` + `cur <= nxt;` on the clock edge |
| `MOV rd, rs` | `assign rd = rs;` |
| `ADD rd, a, b` | `assign rd = a + b;` |
| `SUB rd, a, b` | `assign rd = a - b;` |
| `MUL rd, a, b` | `assign rd = a * b;` |
| `AND rd, a, b` | `assign rd = a & b;` |
| `OR rd, a, b` | `assign rd = a \| b;` |
| `XOR rd, a, b` | `assign rd = a ^ b;` |
| `SEQ rd, a, b` | `assign rd = a == b;` |
| `MUX rd, sel, f, t` | `assign rd = sel ? t : f;` (note: MASM lists the *false* value first) |
| `PADZERO rd, rs, 8` | `assign rd = {{N{1'b0}}, rs};` |
| `(0) FINISH c` | `` `ifndef SYNTHESIS `` `always @(*) if (c) $finish;` `` `endif `` |
| `@DEBUGSYMBOL [ symbol = "x" ]` before a def | `(* keep *) wire x = <that def>;` (skipped with a warning if `x` is not a plain identifier or already used) |
| other annotations, e.g. `@MEMBLOCK [ block = "mem", width = 8, capacity = 16 ]` | parsed, then ignored (any number of `key = value` pairs, string or integer values; the `[...]` is optional) |

**Numbers** can be decimal (`12`), binary (`0b1100`) or hex (`0xc`). Constants and
register init values can be any width, also over 64 bits; widths and the `FINISH`
order must fit in 32 bits, and integer annotation values in 64 bits.

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
  (memories, shifts, SLT/SLTS, SLICE, PARMUX, `$display`, ...) is a parse error.
- Annotations are parsed, but only `@DEBUGSYMBOL` is kept.
- A `.const` value that does not fit in its declared width is an error.
- The test designs need a `$finish`: without a system call the frontend removes all
  logic (`Main` has no outputs) and stops with "Could not detect the clock!".
  The frontend defines `SYNTHESIS`, so that `$finish` must not be inside
  `` `ifndef SYNTHESIS ``. `alu.sv` uses `` `ifndef EQY `` instead (see `alu.eqy`).
  The committed `stepper.sv` has the `SYNTHESIS` guard, which was added after
  `stepper.masm` was made; it cannot be given to the frontend as it is.
- One clock, named `clock`; module name is `Main`.

### `$finish` and EQY

- **Not checked by EQY.** `$finish` is a simulation command, not hardware. Yosys's
  `read_verilog` defines `SYNTHESIS`, and both `stepper.sv` and `generated.sv` put the
  `$finish` inside `` `ifndef SYNTHESIS ``, so EQY never sees it. (Changing the finish
  condition in `generated.sv` still gives an EQY PASS.) `alu.sv` guards its `$finish`
  with `` `ifndef EQY `` and `alu.eqy` reads it with `-DEQY`, with the same effect.
- `scripts/sim_check.sh` only simulates stepper so far.
- **Checked by simulation instead.** `scripts/sim_check.sh` runs both designs in Icarus
  Verilog and compares `step`/`total` every cycle and the time `$finish` is called. It
  also checks that a broken finish condition is detected.
- The guard is now printed by the generator itself, so `generated.sv` is exactly what
  `cargo run` produces (CI regenerates it and fails if the committed file differs).
