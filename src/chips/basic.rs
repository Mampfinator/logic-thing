use std::hash::{Hash, Hasher};

use macroquad::{
    color::{BLACK, Color, WHITE},
    math::{uvec2, vec2},
    shapes::{draw_circle, draw_circle_lines},
    text::draw_text,
};

use crate::{
    TILE_SIZE,
    game_objects::{GameObject, GameObjects, GetState, ObjectContext, ObjectContextMut},
    impl_mgo,
    simulation::{Chip, ChipId, Pin, PinDef, PinLayout, PinsState, Simulation},
};

pub struct TieHigh;

impl Chip for TieHigh {
    fn setup(&mut self) -> PinLayout {
        PinLayout::new_with(
            uvec2(1, 1),
            [(Pin::Right(0), PinDef::new_with_state("HIGH", true))],
        )
    }

    fn update(&mut self, _: &mut PinsState) {}
}

pub struct Clock {
    current_tick: usize,
    interval: usize,
}

impl Clock {
    pub fn new(interval: usize) -> Self {
        Self {
            current_tick: 0,
            interval,
        }
    }
}

impl Chip for Clock {
    fn setup(&mut self) -> PinLayout {
        let mut layout = PinLayout::new(1, 2);
        layout.set(
            Pin::Right(0),
            PinDef {
                label: Some("CLKB".into()),
                initial_state: true,
            },
        );
        layout.set(
            Pin::Right(1),
            PinDef {
                label: Some("CLK".into()),
                initial_state: false,
            },
        );
        layout
    }

    fn update(&mut self, state: &mut PinsState) {
        self.current_tick += 1;
        if self.current_tick.is_multiple_of(self.interval) {
            state.toggle(Pin::Right(0));
            state.toggle(Pin::Right(1));
        }
    }
}

pub struct Nand {
    gates: usize,
}

impl Nand {
    pub fn new(gates: usize) -> Self {
        Self { gates }
    }
}

impl Chip for Nand {
    fn setup(&mut self) -> PinLayout {
        let mut layout = PinLayout::new(2, self.gates * 2);
        for i in 0..self.gates {
            layout.set(Pin::Left(2 * i), PinDef::new(format!("IN{}", 2 * i)));
            layout.set(
                Pin::Left(2 * i + 1),
                PinDef::new(format!("IN{}", 2 * i + 1)),
            );
            layout.set(Pin::Right(2 * i), PinDef::new(format!("OUT{i}")));
        }
        layout
    }

    fn update(&mut self, state: &mut PinsState) {
        for i in 0..self.gates {
            let a = state.read_wire(Pin::Left(2 * i)).is_high();
            let b = state.read_wire(Pin::Left(2 * i + 1)).is_high();
            state.set(Pin::Right(i * 2), !(a && b));
        }
    }
}

#[derive(Default)]
pub struct Counter8b {
    count: u8,
}

impl Chip for Counter8b {
    fn setup(&mut self) -> PinLayout {
        PinLayout::new_with(
            uvec2(2, 8),
            [
                (Pin::Left(4), PinDef::new("CLK")),
                (Pin::Right(0), PinDef::new("C0")),
                (Pin::Right(1), PinDef::new("C1")),
                (Pin::Right(2), PinDef::new("C2")),
                (Pin::Right(3), PinDef::new("C3")),
                (Pin::Right(4), PinDef::new("C4")),
                (Pin::Right(5), PinDef::new("C5")),
                (Pin::Right(6), PinDef::new("C6")),
                (Pin::Right(7), PinDef::new("C7")),
            ],
        )
    }

    fn update(&mut self, state: &mut PinsState) {
        if !state.read_wire(Pin::Left(4)).is_falling_edge() {
            return;
        }
        self.count = self.count.wrapping_add(1);
        for i in 0..8u8 {
            state.set(Pin::Right(i as usize), self.count & 1u8 << i > 0);
        }
    }
}

pub struct Led;

impl Chip for Led {
    fn setup(&mut self) -> PinLayout {
        PinLayout::new_with(uvec2(1, 1), [(Pin::Left(0), PinDef::new("ON"))])
    }

    fn update(&mut self, _: &mut PinsState) {}
}

#[derive(PartialEq)]
pub struct LedObj(ChipId, Color);

impl Hash for LedObj {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);

        // Reinterpret the four color components as one value for object identity.
        let color: u128 = unsafe { std::mem::transmute([self.1.r, self.1.g, self.1.b, self.1.a]) };
        color.hash(state);
    }
}

impl LedObj {
    pub fn new(chip: ChipId, color: Color) -> Self {
        Self(chip, color)
    }
}

impl GameObject for LedObj {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        let pin = simulation
            .chips
            .get(self.0)
            .unwrap()
            .pins_as_positions()
            .next()
            .unwrap()
            .1;
        ctx.spawn_child(pin, ctx.position() - vec2(TILE_SIZE, 0.));
    }

    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_mouse_exit(ctx, simulation);
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.update(ctx, simulation);
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, _: &GameObjects) {
        let chip = simulation.chips.get(self.0).unwrap();
        let pin = chip.get_pinid(Pin::Left(0)).unwrap();
        let Some(network) = simulation.networks.get_network(pin) else {
            return;
        };
        let color = if simulation.networks.get_state(network).unwrap().is_high() {
            self.1
        } else {
            Color::from_rgba(255, 255, 255, 100)
        };
        let position = ctx.position();
        draw_circle(position.x, position.y, TILE_SIZE, color);
        draw_circle_lines(position.x, position.y, TILE_SIZE, 1., BLACK);
    }
}

pub struct NumericDisplay;

impl Chip for NumericDisplay {
    fn setup(&mut self) -> PinLayout {
        PinLayout::new_with(
            uvec2(8, 4),
            (0..8).map(|index| (Pin::Top(index), PinDef::new(format!("C{index}")))),
        )
    }

    fn update(&mut self, _: &mut PinsState) {}
}

#[derive(PartialEq, Hash)]
pub struct NumericDisplayObj(ChipId);

impl From<ChipId> for NumericDisplayObj {
    fn from(value: ChipId) -> Self {
        Self(value)
    }
}

impl GameObject for NumericDisplayObj {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        self.0.start(ctx, simulation);
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_click(ctx, simulation);
    }

    fn on_click_released(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_click_released(ctx, simulation);
    }

    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_mouse_exit(ctx, simulation);
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        if simulation.chips.get(self.0).is_none() {
            ctx.despawn();
            return;
        }
        self.0.update(ctx, simulation);
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        self.0.render(ctx, simulation, objects);
        let instance = simulation.chips.get(self.0).unwrap();
        let number = instance
            .pins
            .iter()
            .filter_map(|pin| *pin)
            .enumerate()
            .filter_map(|(index, id)| {
                simulation.networks.get_network(id).and_then(|network| {
                    simulation
                        .networks
                        .get_state(network)
                        .map(|state| (index, state))
                })
            })
            .fold(0_u8, |value, (index, state)| {
                value | (state.is_high() as u8) << index
            });
        let chip_center = ctx.position() + instance.size.as_vec2() * TILE_SIZE / 2.;
        draw_text(
            &number.to_string(),
            ctx.position().x + TILE_SIZE,
            chip_center.y + 12.,
            56.,
            WHITE,
        );
    }
}

impl_mgo!(
    Clock,
    Counter8b,
    TieHigh,
    Led as LedObj where Args = (Color),
    NumericDisplay as NumericDisplayObj,
    Nand,
);
