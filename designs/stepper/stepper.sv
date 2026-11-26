module Main (input wire clock);

    reg [3:0] step = 0;
    reg [3:0] total = 0;

    always @(posedge clock) begin
        step <= step + 1;
        if (step == 3)
            total <= total + 2;
    `ifndef SYNTHESIS
        if (total == 6)
            $finish;
    `endif
    end
endmodule