let clock = place_clock(10, ivec2(0, 1));
let high = place_high(ivec2(0, 0));

let cpu = place_cpu(ivec2(10, 0));

connect(high, right(0), cpu, "CE");
connect(clock, "CLK", cpu, "CLK");

let rom = place_default_rom(ivec2(20, 0));

connect(high, right(0), rom, "CE");
connect(clock, "CLK", rom, "CLK");

for i in 0..8 {
    // connect address pins
    connect(rom, left(i + 1), cpu, right(i));
    // connect data pins
    connect(rom, right(i + 1), cpu, left(i + 4));
}