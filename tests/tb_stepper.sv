// Testbench for scripts/sim_check.sh: runs either stepper.sv or generated.sv
// (both define module Main) and prints step/total every cycle. The gate design
// has `step`/`total` too: the aliases printed from @DEBUGSYMBOL.
`timescale 1ns/1ns
module tb;
    reg clock = 0;
    Main dut (.clock(clock));

    always #5 clock = ~clock; // rising edges at 5, 15, 25, ...

    // sample halfway between edges, when everything is stable
    always @(negedge clock)
        $display("t=%0t step=%0d total=%0d", $time, dut.step, dut.total);

    initial begin
        #1000;
        $display("TIMEOUT: $finish was never called");
        $finish;
    end
endmodule
