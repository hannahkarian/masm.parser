#!/usr/bin/env bash
# EQY check for the stepper example, with a negative control.
#
#   1. regenerate generated.sv from stepper.masm
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
run_eqy() { # $1 = name, $2 = gate .sv file
    local dir=$BUILD/$1
    mkdir -p "$dir"
    cp stepper.sv stepper.eqy "$dir/"
    cp "$2" "$dir/generated.sv"
    (cd "$dir" && eqy -f stepper.eqy > eqy.out 2>&1)
    local rc=$?
    local parts
    parts=$(wc -l < "$dir/stepper/partition.list" 2>/dev/null || echo 0)
    if [ $rc -eq 0 ] && grep -q "DONE (PASS" "$dir/eqy.out"; then
        echo "PASS $parts"
    elif grep -q "DONE (FAIL" "$dir/eqy.out"; then
        echo "FAIL $parts"
    else
        echo "ERROR $parts"
    fi
}

echo "== 1. generate generated.sv"
cargo run -q -- stepper.masm generated.sv || { echo "cargo run failed"; exit 1; }

echo "== 2. EQY: stepper.sv vs generated.sv (expect PASS)"
read -r result parts <<< "$(run_eqy original generated.sv)"
echo "  result: $result, partitions: $parts"
[ "$result" = PASS ] || fail "original design did not pass (see $BUILD/original/eqy.out)"
[ "$parts" -gt 0 ] || fail "0 partitions: nothing was compared (vacuous pass)"

echo "== 3. EQY on broken copies of generated.sv (expect FAIL)"
# name | sed expression applied to generated.sv
mutants=(
    "add_becomes_sub|s/ys_w5 + ys_w6/ys_w5 - ys_w6/"
    "mux_arms_swapped|s/ys_w10 ? ys_w9 : ys_w3/ys_w10 ? ys_w3 : ys_w9/"
    "compare_with_2_not_3|s/ys_c3 = 2'b11/ys_c3 = 2'b10/"
    "add_1_not_2|s/ys_c1 = 2'b10/ys_c1 = 2'b01/"
    "padzero_wrong_bits|s/{{2{1'b0}}, ys_c1}/{ys_c1, {2{1'b0}}}/"
    "register_not_updated|s/ys_i1 <= ys_o2;/ys_i1 <= ys_w3;/"
    "wrong_initial_value|s/reg \\[3:0\\] ys_i15 = 4'b0000/reg [3:0] ys_i15 = 4'b0101/"
)
# Not in this list on purpose: a wrong $finish condition. $finish is removed under
# SYNTHESIS in both designs, so EQY cannot see it. scripts/sim_check.sh covers that.
for m in "${mutants[@]}"; do
    name=${m%%|*}
    expr=${m#*|}
    src=$BUILD/$name.sv
    sed "$expr" generated.sv > "$src"
    if cmp -s generated.sv "$src"; then
        fail "$name: sed did not change anything (update the mutant)"
        continue
    fi
    read -r result parts <<< "$(run_eqy "$name" "$src")"
    printf '  %-24s -> %s\n' "$name" "$result"
    [ "$result" = FAIL ] || fail "$name: broken design was not detected (got $result)"
done

echo
if [ $failures -eq 0 ]; then
    echo "ALL OK: real design PASSES, every broken design FAILS"
else
    echo "$failures problem(s) found"
    exit 1
fi
