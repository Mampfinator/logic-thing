use macroquad::{
    color::RED,
    input::{self, KeyCode},
    math::{Circle, uvec2, vec2},
    shapes::draw_circle,
};

use crate::{
    GameObject, GameObjects, GetState, ObjectContext, ObjectContextMut, TILE_SIZE, impl_mgo,
    simulation::{self, Chip, ChipId, Pin, PinDef, PinId, PinLayout, PinsState, Simulation},
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

#[derive(PartialEq, Hash)]
struct ButtonInner(PinId);

impl GameObject for ButtonInner {
    fn start(&mut self, ctx: &mut ObjectContextMut, _: &Simulation) {
        let center = ctx.position() + vec2(TILE_SIZE, TILE_SIZE) / 2.;
        ctx.set_shape(crate::game_objects::Shape::Circle(Circle {
            x: center.x,
            y: center.y,
            r: TILE_SIZE / 6. * 2.,
        }));
    }

    fn on_click(&mut self, _: &mut ObjectContextMut, simulation: &mut Simulation) {
        if input::is_key_down(KeyCode::LeftAlt) {
            return;
        }

        let Some(pin) = simulation.pins.get_mut(self.0) else {
            return;
        };

        pin.state = true;
    }

    fn on_click_released(&mut self, _: &mut ObjectContextMut, simulation: &mut Simulation) {
        let Some(pin) = simulation.pins.get_mut(self.0) else {
            return;
        };

        pin.state = false;
    }

    fn render(&self, context: &ObjectContext, _: &Simulation, _: &GameObjects) {
        let center = context.position() + vec2(TILE_SIZE, TILE_SIZE) / 2.;
        draw_circle(center.x, center.y, TILE_SIZE / 6. * 2., RED);
    }
}

impl GameObject for ButtonObj {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &simulation::Simulation) {
        self.chip.start(ctx, simulation);
        let chip = simulation.chips.get(self.chip).unwrap();
        ctx.spawn_child(
            ButtonInner(chip.get_pinid(Pin::Right(0)).unwrap()),
            ctx.position(),
        );
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.chip.on_click(ctx, simulation);
    }

    fn on_click_released(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.chip.on_click_released(ctx, simulation);
    }

    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.chip.on_mouse_exit(ctx, simulation);
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        if simulation.chips.get(self.chip).is_none() {
            ctx.despawn();
            return;
        }
        self.chip.update(ctx, simulation);
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        self.chip.render(ctx, simulation, objects);
        let center = ctx.position() + vec2(TILE_SIZE, TILE_SIZE) / 2.;
        draw_circle(center.x, center.y, TILE_SIZE / 6. * 2., RED);
    }
}
