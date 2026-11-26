#!/usr/bin/env bash
# Simulation check for what EQY cannot see: $finish (and the run as a whole).
# Runs stepper.sv and stepper_generated.sv with tb_stepper.sv (all in designs/stepper/)
# in Icarus Verilog and compares the printed values every cycle plus the time
# $finish is called.
# A broken $finish condition in a copy of stepper_generated.sv must make them differ.
#
# Usage: scripts/sim_check.sh     (needs cargo and iverilog)

set -u
cd "$(dirname "$0")/.."
BUILD=build/sim
D=designs/stepper
rm -rf "$BUILD" && mkdir -p "$BUILD"

# simulate one design; output = the trace with the file:line of $finish removed
sim() { # $1 = design .sv, $2 = output name
    iverilog -g2012 -o "$BUILD/$2.vvp" "$D/tb_stepper.sv" "$1" || exit 1
    vvp -n "$BUILD/$2.vvp" | sed 's/^.*\$finish called at/$finish called at/' > "$BUILD/$2.txt"
}

cargo run -q -- "$D/stepper.masm" "$D/stepper_generated.sv" || exit 1
sim "$D/stepper.sv" gold
sim "$D/stepper_generated.sv" gate

status=0
if diff -u "$BUILD/gold.txt" "$BUILD/gate.txt"; then
    echo "OK: same trace and same \$finish time ($(grep -o 'finish called at [0-9]*' "$BUILD/gold.txt"))"
else
    echo "FAIL: stepper.sv and stepper_generated.sv simulate differently"; status=1
fi
grep -q TIMEOUT "$BUILD/gold.txt" && { echo "FAIL: \$finish never called"; status=1; }

# negative control: wrong $finish condition (total != 6 instead of total == 6)
sed 's/ys_w3 == ys_w12/ys_w3 != ys_w12/' "$D/stepper_generated.sv" > "$BUILD/finish_mutant.sv"
sim "$BUILD/finish_mutant.sv" mutant
if cmp -s "$BUILD/gold.txt" "$BUILD/mutant.txt"; then
    echo "FAIL: broken \$finish condition was not detected"; status=1
else
    echo "OK: broken \$finish condition is detected"
fi
exit $status
