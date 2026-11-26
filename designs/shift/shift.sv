// Test design for the shifts and comparisons: SLL, SRL, SRA, SLT, SLTS.
// `a` and `b` are two counters with different steps and `n` is a 3-bit shift amount,
// so the operations see many different operand pairs, including negative values for
// the signed ones (a 4-bit value is negative when its top bit is 1) and shift
// amounts larger than the width (4..7), which shift everything out.
// The shift amount is a register, not a constant: Yosys turns a shift by a constant
// into plain wiring, and then no SLL/SRL/SRA would reach the MASM.
// Every result is stored in its own register, which gives EQY one named signal per
// operation to compare.
//
// The $finish is needed to get any MASM at all (see alu.sv); its condition uses
// every result. The guard is `EQY` and not `SYNTHESIS` for the same reason as there.
module Main (input wire clock);

    reg [3:0] a = 4'd3;
    reg [3:0] b = 4'd5;
    reg [2:0] n = 3'd0;

    reg [3:0] shl = 0;    // a << n                      SLL
    reg [3:0] shr = 0;    // a >> n                      SRL
    reg [3:0] ashr = 0;   // a >>> n, a read as signed   SRA
    reg less = 0;         // a < b, unsigned             SLT
    reg less_s = 0;       // a < b, signed               SLTS

    always @(posedge clock) begin
        a <= a + 4'd1;
        b <= b + 4'd3;
        n <= n + 3'd1;

        shl    <= a << n;
        shr    <= a >> n;
        ashr   <= $signed(a) >>> n;
        less   <= a < b;
        less_s <= $signed(a) < $signed(b);
    `ifndef EQY
        if ((shl ^ shr ^ ashr) == 4'd9 && less && !less_s)
            $finish;
    `endif
    end
endmodule
