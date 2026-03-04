use macroquad::{
    color::RED,
    input,
    math::{Circle, uvec2, vec2},
    shapes::draw_circle,
};

use crate::{
    GameObject, GameObjects, GetState, ObjectContext, ObjectContextMut, TILE_SIZE, impl_mgo,
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

#[derive(Hash)]
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
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &simulation::Simulation) {
        self.chip.start(ctx, simulation);
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        let center = ctx.position() + vec2(TILE_SIZE, TILE_SIZE) / 2.;
        let area = Circle::new(center.x, center.y, TILE_SIZE / 3. * 2.);

        let world_pos = ctx.mouse_world_pos();

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

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        self.chip.render(ctx, simulation, objects);
        let center = ctx.position() + vec2(TILE_SIZE, TILE_SIZE) / 2.;
        draw_circle(center.x, center.y, TILE_SIZE / 6. * 2., RED);
    }
}
