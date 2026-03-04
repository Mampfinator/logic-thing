use macroquad::{
    color::{BLACK, RED, WHITE},
    input,
    math::{Rect, uvec2, vec2},
    shapes::draw_rectangle,
};

use crate::{GetState, ObjectContext, ObjectContextMut, impl_mgo};

use crate::{
    GameObject, GameObjects, TILE_SIZE,
    simulation::{Chip, ChipId, Pin, PinDef, PinLayout, PinsState, Simulation},
};

pub struct Switch {
    switches: usize,
}

impl Switch {
    pub fn new(switches: usize) -> Self {
        Self { switches }
    }
}

impl Chip for Switch {
    fn setup(&self) -> crate::simulation::PinLayout {
        PinLayout::new_with(
            uvec2(4, self.switches as u32),
            (0..self.switches).map(|i| (Pin::Right(i), PinDef::new(format!("S{i}")))),
        )
    }

    fn update(&mut self, _state: &mut PinsState) {}
}

impl_mgo!(Switch as SwitchObj where Args = (usize));

#[derive(Hash)]
pub struct SwitchObj {
    chip: ChipId,
    switches: Vec<bool>,
}

impl SwitchObj {
    pub fn new(chip: ChipId, switches: usize) -> Self {
        Self {
            chip,
            switches: std::iter::repeat_n(false, switches).collect(),
        }
    }
}

impl GameObject for SwitchObj {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        self.chip.start(ctx, simulation);
    }

    fn update(&mut self, state: &mut ObjectContextMut, simulation: &mut Simulation) {
        if !input::is_mouse_button_pressed(input::MouseButton::Left) {
            return;
        }

        let mouse_pos = state.mouse_world_pos();

        for (switch, switch_state) in self.switches.iter_mut().enumerate() {
            let pos = state.position() + vec2(TILE_SIZE / 2., TILE_SIZE * switch as f32 + 1.);

            if Rect::new(pos.x, pos.y, TILE_SIZE * 2., TILE_SIZE - 2.).contains(mouse_pos) {
                *switch_state = !*switch_state;
                let chip = simulation.chips.get(self.chip).unwrap();
                let pin = simulation.pins.get_mut(chip.pins[switch].unwrap()).unwrap();
                pin.state = !pin.state;
            }
        }
    }

    fn render(&self, state: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        self.chip.render(state, simulation, objects);
        for (switch, switch_state) in self.switches.iter().copied().enumerate() {
            let switch_pos = state.position() + vec2(TILE_SIZE, TILE_SIZE * switch as f32 + 1.);
            let color = if switch_state { RED } else { BLACK };

            let switch_x = if switch_state { TILE_SIZE * 1.5 } else { 0. };

            draw_rectangle(
                switch_pos.x,
                switch_pos.y,
                TILE_SIZE * 2.,
                TILE_SIZE - 2.,
                color,
            );

            draw_rectangle(
                switch_pos.x + switch_x,
                switch_pos.y,
                TILE_SIZE / 2.,
                TILE_SIZE - 2.,
                WHITE,
            );
        }
    }
}
