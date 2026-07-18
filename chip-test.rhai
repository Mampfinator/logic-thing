fn setup() {
    make_layout(uvec2(3, 3), [
        [top(1), pin_with_state("T", true)],
        [right(1), pin("R")],
        [bottom(1), pin("B")],
        [left(1), pin("L")],
    ])
}

let PINS = ["T", "R", "B", "L"];

fn update(pins, state) {
    if !state.has("tick") {
        state.set("tick", 0);
    }

    state.set("tick", state.get("tick") + 1);

    let tick = state.get("tick");

    let activate_pin = PINS[tick % 4];
    let deactivate_pin = PINS[(tick - 1) % 4];

    mutations()
        .mutate(pins.set(activate_pin, true))
        .mutate(pins.set(deactivate_pin, false))
}