fn setup() {
    make_layout(uvec2(3, 3), [
        [top(1), pin_with_state("T", true)],
        [right(1), pin("R")],
        [bottom(1), pin("B")],
        [left(1), pin("L")],
    ])
}

fn update(state) {
    if 1 == 1 {
        mutations().mutate(state.toggle("T"))
    }
}