use std::iter;

use macroquad::{
    color::{BLACK, ORANGE, RED, WHITE},
    input,
    math::{Rect, uvec2, vec2},
    shapes::draw_rectangle,
};

use crate::impl_mgo;

use crate::{
    Camera, GameObject, GameObjectState, GameObjects, Shape, TILE_SIZE,
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

    fn update(&mut self, state: &mut PinsState) {}
}

impl_mgo!(Switch as SwitchObj where Args: (usize));

pub struct SwitchObj {
    chip: ChipId,
    switches: Vec<bool>,
}

impl SwitchObj {
    pub fn new(chip: ChipId, switches: usize) -> Self {
        Self {
            chip,
            switches: iter::repeat(false).take(switches).collect(),
        }
    }
}

impl GameObject for SwitchObj {
    fn start(
        &mut self,
        state: &mut GameObjectState,
        simulation: &Simulation,
        objects: &mut GameObjects,
    ) {
        self.chip.start(state, simulation, objects);
    }

    fn update(
        &mut self,
        state: &mut GameObjectState,
        simulation: &mut Simulation,
        camera: &mut Camera,
    ) {
        if !input::is_mouse_button_pressed(input::MouseButton::Left) {
            return;
        }

        let pos = input::mouse_position();
        let mouse_pos = camera.camera.screen_to_world(vec2(pos.0, pos.1));

        for (switch, switch_state) in self.switches.iter_mut().enumerate() {
            let pos = state.position + vec2(TILE_SIZE / 2., TILE_SIZE * switch as f32 + 1.);

            if Rect::new(pos.x, pos.y, TILE_SIZE * 2., TILE_SIZE - 2.).contains(mouse_pos) {
                *switch_state = !*switch_state;
                let chip = simulation.chips.get(self.chip).unwrap();
                let pin = simulation.pins.get_mut(chip.pins[switch].unwrap()).unwrap();
                pin.state = !pin.state;
            }
        }
    }

    fn render(&self, state: &GameObjectState, simulation: &Simulation, objects: &GameObjects) {
        self.chip.render(state, simulation, objects);
        for (switch, switch_state) in self.switches.iter().copied().enumerate() {
            let switch_pos = state.position + vec2(TILE_SIZE, TILE_SIZE * switch as f32 + 1.);
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
