// Test design for shifts and comparisons whose operands have *different* widths.
// designs/shift/ only has equal widths; this one makes the frontend write the
// mixed-width forms, which is where the width rules in verilog.rs matter:
//   narrow    4-bit a >> n into 2 bits     SRL with rd narrower than rs1
//   wide_shl  4-bit a << n into 6 bits     SLL with rd wider than rs1
//   cat       {a, n, 1'b1}                 SLL by 32-bit constants (concat)
//   gt, ge    a > c, a >= c (4 vs 6 bits)  SLT with swapped operands, SLT + SEQ
//   lts_mixed signed 4 vs 6 bits           sign extension: SRL of a 4-bit value by a
//                                          32-bit constant into 1 bit, then SLTS
// SLL/SRL have no width rule (the result is clipped to rd), so these must pass
// EQY without a warning.
//
// The $finish condition must read *every bit* of every result: the frontend
// narrows a register whose upper bits nothing reads and writes those bits as
// constant 0, while EQY keeps them in the gold design (so EQY would fail).
// The guard is `EQY` and not `SYNTHESIS` for the same reason as in alu.sv.
module Main (input wire clock);

    reg [3:0] a = 4'd3;
    reg [5:0] c = 6'd9;
    reg [2:0] n = 3'd0;

    reg [1:0] narrow = 0;
    reg [7:0] cat = 0;
    reg gt = 0, ge = 0;
    reg lts_mixed = 0;
    reg [5:0] wide_shl = 0;

    always @(posedge clock) begin
        a <= a + 4'd1;
        c <= c + 6'd5;
        n <= n + 3'd1;

        narrow    <= a >> n;
        cat       <= {a, n, 1'b1};
        gt        <= a > c;
        ge        <= a >= c;
        lts_mixed <= $signed(a) < $signed(c);
        wide_shl  <= a << n;
    `ifndef EQY
        if ((cat ^ {2'b0, wide_shl} ^ {6'b0, narrow}) == 8'd3 && gt && ge && lts_mixed)
            $finish;
    `endif
    end
endmodule
