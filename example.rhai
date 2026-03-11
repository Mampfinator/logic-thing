let clock = place_clock(10, ivec2(0, 1));
let high = place_high(ivec2(0, 0));

let cpu = place_cpu(ivec2(10, 0));

connect(high, right(0), cpu, "CE");


connect(clock, "CLK", cpu, "CLK");