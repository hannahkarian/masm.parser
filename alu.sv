// Test design for the plain ALU operations: SUB, MUL, AND, OR, XOR.
// `a` and `b` are two counters with different steps, so the operations see many
// different operand pairs. Every result is stored in its own register, which gives
// EQY one named signal per operation to compare.
//
// The $finish is needed to get any MASM at all: the frontend deletes logic that no
// system call depends on, and Main has no outputs. Its condition uses every result.
//
// The guard is `EQY` and not `SYNTHESIS`, because the Manticore frontend is a Yosys
// too and defines SYNTHESIS: it would skip the $finish and delete the whole design.
// alu.eqy passes -DEQY, so this one file works for the frontend, EQY and simulation.
module Main (input wire clock);

    reg [3:0] a = 4'd3;
    reg [3:0] b = 4'd5;

    reg [3:0] diff = 0;
    reg [3:0] prod = 0;
    reg [3:0] both = 0;
    reg [3:0] either = 0;
    reg [3:0] differ = 0;

    always @(posedge clock) begin
        a <= a + 4'd1;
        b <= b + 4'd3;

        diff   <= a - b;
        prod   <= a * b;
        both   <= a & b;
        either <= a | b;
        differ <= a ^ b;
    `ifndef EQY
        if ((diff ^ prod ^ both ^ either ^ differ) == 4'd9)
            $finish;
    `endif
    end
endmodule
