always @(posedge clock) begin
    flag <= ~flag;
    if (flag) $finish;
end