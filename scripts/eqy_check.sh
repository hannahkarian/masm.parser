#!/usr/bin/env bash
# EQY check for every test design, with a negative control.
#
# For each design (stepper, alu):
#   1. regenerate the gate file from the .masm
#   2. EQY on the real design            -> must PASS, with at least one partition
#   3. EQY on deliberately broken copies -> every one must FAIL
#
# Step 3 is what shows the PASS in step 2 means something: if a broken design
# also passes, EQY is not really comparing the logic (that was the old bug:
# all logic was optimised away and EQY compared two empty modules).
#
# Usage: scripts/eqy_check.sh        (from anywhere; needs cargo, eqy, yosys, sby)
# Work directories are written to build/eqy/ (ignored by git).

set -u
cd "$(dirname "$0")/.."
ROOT=$PWD
BUILD=$ROOT/build/eqy
rm -rf "$BUILD" && mkdir -p "$BUILD"

failures=0
fail() { echo "  FAIL: $*"; failures=$((failures + 1)); }

# run EQY on a given gate file in its own directory; prints PASS/FAIL/ERROR
run_eqy() { # $1 = design, $2 = gate file name in the .eqy, $3 = run name, $4 = gate .sv file
    local dir=$BUILD/$1/$3
    mkdir -p "$dir"
    cp "$1.sv" "$1.eqy" "$dir/"
    cp "$4" "$dir/$2"
    (cd "$dir" && eqy -f "$1.eqy" > eqy.out 2>&1)
    local rc=$?
    local parts
    parts=$(wc -l < "$dir/$1/partition.list" 2>/dev/null || echo 0)
    if [ $rc -eq 0 ] && grep -q "DONE (PASS" "$dir/eqy.out"; then
        echo "PASS $parts"
    elif grep -q "DONE (FAIL" "$dir/eqy.out"; then
        echo "FAIL $parts"
    else
        echo "ERROR $parts"
    fi
}

# the three steps for one design
check_design() { # $1 = design, $2 = gate file, $3... = mutants: "name|sed expression"
    local design=$1 gate=$2
    shift 2
    local result parts m name expr src

    echo "== $design: generate $gate"
    cargo run -q -- "$design.masm" "$gate" || { fail "$design: cargo run failed"; return; }

    echo "== $design: EQY $design.sv vs $gate (expect PASS)"
    read -r result parts <<< "$(run_eqy "$design" "$gate" original "$gate")"
    echo "  result: $result, partitions: $parts"
    [ "$result" = PASS ] || fail "$design did not pass (see $BUILD/$design/original/eqy.out)"
    [ "$parts" -gt 0 ] || fail "$design: 0 partitions, nothing was compared (vacuous pass)"

    echo "== $design: EQY on broken copies of $gate (expect FAIL)"
    for m in "$@"; do
        name=${m%%|*}
        expr=${m#*|}
        src=$BUILD/$design/$name.sv
        sed "$expr" "$gate" > "$src"
        if cmp -s "$gate" "$src"; then
            fail "$design/$name: sed did not change anything (update the mutant)"
            continue
        fi
        read -r result parts <<< "$(run_eqy "$design" "$gate" "$name" "$src")"
        printf '  %-24s -> %s\n' "$name" "$result"
        [ "$result" = FAIL ] || fail "$design/$name: broken design was not detected (got $result)"
    done
    echo
}

# Not in these lists on purpose: a wrong $finish condition. $finish is removed for
# EQY in both designs, so EQY cannot see it. scripts/sim_check.sh covers that.
check_design stepper generated.sv \
    "add_becomes_sub|s/ys_w5 + ys_w6/ys_w5 - ys_w6/" \
    "mux_arms_swapped|s/ys_w10 ? ys_w9 : ys_w3/ys_w10 ? ys_w3 : ys_w9/" \
    "compare_with_2_not_3|s/ys_c3 = 2'b11/ys_c3 = 2'b10/" \
    "add_1_not_2|s/ys_c1 = 2'b10/ys_c1 = 2'b01/" \
    "padzero_wrong_bits|s/{{2{1'b0}}, ys_c1}/{ys_c1, {2{1'b0}}}/" \
    "register_not_updated|s/ys_i1 <= ys_o2;/ys_i1 <= ys_w3;/" \
    "wrong_initial_value|s/reg \\[3:0\\] ys_i15 = 4'b0000/reg [3:0] ys_i15 = 4'b0101/"

# One mutant per new operation, plus swapped SUB operands (the only one of the five
# where the operand order matters).
check_design alu alu_generated.sv \
    "sub_becomes_add|s/ys_w0 - ys_w3/ys_w0 + ys_w3/" \
    "sub_operands_swapped|s/ys_w0 - ys_w3/ys_w3 - ys_w0/" \
    "mul_becomes_add|s/ys_w0 \\* ys_w3/ys_w0 + ys_w3/" \
    "and_becomes_or|s/ys_w0 & ys_w3/ys_w0 | ys_w3/" \
    "or_becomes_xor|s/ys_w0 | ys_w3/ys_w0 ^ ys_w3/" \
    "xor_becomes_or|s/ys_w0 ^ ys_w3/ys_w0 | ys_w3/"

if [ $failures -eq 0 ]; then
    echo "ALL OK: every real design PASSES, every broken design FAILS"
else
    echo "$failures problem(s) found"
    exit 1
fi
