use macroquad::{
    color::{BLACK, RED, WHITE},
    input::{self, KeyCode},
    math::{Rect, uvec2, vec2},
    shapes::draw_rectangle,
};

use crate::{
    GetState, ObjectContext, ObjectContextMut, game_objects::Shape, impl_mgo, simulation::PinId,
};

use crate::{
    GameObject, GameObjects, TILE_SIZE,
    simulation::{Chip, ChipId, Pin, PinDef, PinLayout, PinsState, Simulation},
};

pub struct Switches {
    switches: usize,
}

impl Switches {
    pub fn new(switches: usize) -> Self {
        Self { switches }
    }
}

impl Chip for Switches {
    fn setup(&self) -> crate::simulation::PinLayout {
        PinLayout::new_with(
            uvec2(4, self.switches as u32),
            (0..self.switches).map(|i| (Pin::Right(i), PinDef::new(format!("S{i}")))),
        )
    }

    fn update(&mut self, _state: &mut PinsState) {}
}

impl_mgo!(Switches as SwitchChip where Args = (usize));

#[derive(Hash)]
pub struct SwitchChip {
    chip: ChipId,
    switches: usize,
}

impl SwitchChip {
    pub fn new(chip: ChipId, switches: usize) -> Self {
        Self { chip, switches }
    }
}

#[derive(PartialEq, Hash)]
struct Switch(PinId);

impl GameObject for Switch {
    fn start(&mut self, ctx: &mut ObjectContextMut, _: &Simulation) {
        ctx.set_layer(3);
        ctx.set_shape(Shape::Rectangle(Rect::new(
            ctx.position().x,
            ctx.position().y,
            TILE_SIZE * 2.,
            TILE_SIZE - 2.,
        )));
    }

    fn on_click(&mut self, _: &mut ObjectContextMut, simulation: &mut Simulation) {
        if input::is_key_down(KeyCode::LeftAlt) {
            return;
        }

        let pin = simulation.pins.get_mut(self.0).unwrap();
        pin.state = !pin.state;
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, _: &GameObjects) {
        let state = simulation.pins.get_state(self.0).unwrap_or(false);

        let color = if state { RED } else { BLACK };

        draw_rectangle(
            ctx.position().x,
            ctx.position().y,
            TILE_SIZE * 2.,
            TILE_SIZE - 2.,
            color,
        );

        let switch_x = if state { TILE_SIZE * 1.5 } else { 0. };
        draw_rectangle(
            ctx.position().x + switch_x,
            ctx.position().y,
            TILE_SIZE / 2.,
            TILE_SIZE - 2.,
            WHITE,
        );
    }
}

impl GameObject for SwitchChip {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        self.chip.start(ctx, simulation);

        let instance = simulation.chips.get(self.chip).unwrap();

        for switch in 0..self.switches {
            let pin = instance.get_pinid(Pin::Right(switch)).unwrap();
            ctx.spawn_child(
                Switch(pin),
                ctx.position() + vec2(TILE_SIZE, TILE_SIZE * switch as f32 + 1.),
            );
        }
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
        self.chip.update(ctx, simulation);
    }

    fn render(&self, state: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        self.chip.render(state, simulation, objects);
    }
}
