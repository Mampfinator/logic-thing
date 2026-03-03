use macroquad::{
    color::RED,
    input,
    math::{Circle, uvec2, vec2},
    shapes::draw_circle,
};

use crate::{
    Camera, GameObject, GameObjectState, GameObjects, GetState, ObjectContext, ObjectContextMut,
    TILE_SIZE, impl_mgo,
    simulation::{self, Chip, ChipId, Pin, PinDef, PinLayout, PinsState, Simulation},
};

pub struct Button;

impl Chip for Button {
    fn setup(&self) -> crate::simulation::PinLayout {
        PinLayout::new_with(uvec2(1, 1), [(Pin::Right(0), PinDef::new("OUT"))])
    }

    fn update(&mut self, _: &mut PinsState) {}
}

impl_mgo!(Button as ButtonObj);

pub struct ButtonObj {
    chip: ChipId,
    state: bool,
}

impl From<ChipId> for ButtonObj {
    fn from(value: ChipId) -> Self {
        Self {
            chip: value,
            state: false,
        }
    }
}

impl GameObject for ButtonObj {
    fn start(&mut self, state: &mut ObjectContextMut, simulation: &simulation::Simulation) {
        self.chip.start(state, simulation);
    }

    fn update(
        &mut self,
        state: &mut ObjectContextMut,
        simulation: &mut Simulation,
        camera: &mut Camera,
    ) {
        let center = state.position() + vec2(TILE_SIZE, TILE_SIZE) / 2.;
        let area = Circle::new(center.x, center.y, TILE_SIZE / 3. * 2.);

        let pos = input::mouse_position();
        let world_pos = camera.camera.screen_to_world(vec2(pos.0, pos.1));

        let chip = simulation.chips.get(self.chip).unwrap();
        let pin = simulation.pins.get_mut(chip.pins[0].unwrap()).unwrap();

        if !area.contains(&world_pos) {
            self.state = false;
            pin.state = false;
        } else {
            let state = input::is_mouse_button_down(input::MouseButton::Left);
            self.state = state;
            pin.state = state;
        }
    }

    fn render(&self, state: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        self.chip.render(state, simulation, objects);
        let center = state.position() + vec2(TILE_SIZE, TILE_SIZE) / 2.;
        draw_circle(center.x, center.y, TILE_SIZE / 6. * 2., RED);
    }
}
