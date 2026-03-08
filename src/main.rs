use core::f32;
use std::hash::{Hash, Hasher};

use macroquad::{input, prelude::*};

use crate::{
    chips::{button, rom, switch::Switches},
    game_objects::{
        CommandBuffer, GameObject, GameObjects, GetState, Grid, MakeGameObject, ObjectContext,
        ObjectContextMut, ObjectId, PlaceMgos, Shape, TypeMap,
    },
    simulation::{Chip, ChipId, NetworkId, Pin, PinDef, PinId, PinLayout, PinsState, Simulation},
};

pub const TILE_SIZE: f32 = 16.0;

fn snap_to_grid(position: Vec2, spacing: f32) -> Vec2 {
    vec2(
        (position.x / spacing).round() * spacing,
        (position.y / spacing).round() * spacing,
    )
}

pub mod chips;
pub mod game_objects;
pub mod simulation;

use chips::cpu::{CPU, DATA_PINS};

#[derive(Default)]
struct Game {
    pub simulation: Simulation,
    pub game_objects: GameObjects,
    pub resources: TypeMap,
}

impl Game {
    /// ## Example
    /// ```
    /// let mut game = Game::default()
    ///
    /// let [clock, counter, led] = game.place_chips((
    ///   (Clock::new(100), ivec2(6, 6)),
    ///   (Counter8b, ivec2(12, 6)),
    ///   // this "RED" here is a rendering option. But it can figure it out. :)
    ///   (Led, ivec2(18, 6), RED),
    ///))
    /// ```
    pub fn place_chips<const N: usize, Marker, T: PlaceMgos<Marker, N>>(
        &mut self,
        chips: T,
    ) -> [(ChipId, ObjectId); N] {
        chips.place(self).try_into().unwrap()
    }

    pub fn place_chip<C: MakeGameObject + 'static>(
        &mut self,
        chip: C,
        position: Vec2,
        args: <C as MakeGameObject>::Args,
    ) -> (ChipId, ObjectId)
    where
        <C as MakeGameObject>::Obj: Hash,
    {
        let id = self.simulation.place_chip(chip);

        let object = C::make_game_object(id, args);
        let oid =
            self.game_objects
                .insert(object, position, &mut self.simulation, &mut self.resources);

        (id, oid)
    }

    fn camera(&mut self) -> &mut Camera {
        self.resources.get_mut_or_insert_default()
    }

    pub fn update(&mut self) {
        self.camera().update();

        // process mouse information.
        let mouse_pos = self.camera().get_mouse_world_pos();
        let clicked = input::is_mouse_button_pressed(MouseButton::Left);
        let released = input::is_mouse_button_released(MouseButton::Left);

        let mut buffer = CommandBuffer::default();

        for (id, object, state) in self.game_objects.iter_mut() {
            let Some(is_inside) = state.shape.as_ref().map(|shape| shape.contains(mouse_pos))
            else {
                continue;
            };

            let mut ctx = ObjectContextMut::new(state, id, &mut buffer, &mut self.resources);

            if is_inside && !ctx.hovered() {
                ctx.set_hovered(true);
                object.on_mouse_enter(&mut ctx, &mut self.simulation);
            }

            if !is_inside && ctx.hovered() {
                ctx.set_hovered(false);
                object.on_mouse_exit(&mut ctx, &mut self.simulation);
            }

            if is_inside && clicked {
                object.on_click(&mut ctx, &mut self.simulation);
            }

            if is_inside && released {
                object.on_click_released(&mut ctx, &mut self.simulation);
            }
        }

        buffer.apply(
            &mut self.game_objects,
            &mut self.simulation,
            &mut self.resources,
        );

        for (id, object, state) in self.game_objects.iter_mut() {
            let mut ctx = ObjectContextMut::new(state, id, &mut buffer, &mut self.resources);
            object.update(&mut ctx, &mut self.simulation);
        }

        buffer.apply(
            &mut self.game_objects,
            &mut self.simulation,
            &mut self.resources,
        );
    }

    pub fn render(&self) {
        self.game_objects.render(&self.simulation);
    }
}

impl From<(ChipId, ObjectId)> for ChipId {
    fn from(value: (ChipId, ObjectId)) -> Self {
        value.0
    }
}

impl_mgo!(
    Clock,
    Counter8b,
    TieHigh,
    Led as LedObj where Args = (Color),
    NumericDisplay as NumericDisplayObj,
    CPU,
);

impl GameObject for PinId {
    fn start(&mut self, state: &mut ObjectContextMut, simulation: &Simulation) {
        state.set_layer(2);

        state.set_shape(Shape::Circle(Circle {
            x: state.position().x,
            y: state.position().y,
            r: TILE_SIZE / 4.,
        }));

        let pin = simulation.pins.get(*self).unwrap();

        if let Some(label) = pin.label.clone() {
            let chip = simulation.chips.get(pin.chip).unwrap();
            let (index, _) = chip
                .pins
                .iter()
                .enumerate()
                .find(|(_, pin)| pin.as_ref().map(|other| *other == *self).unwrap_or(false))
                .unwrap();
            let pin_side = Pin::from_index(index, chip.size);

            let (text_offset, rotation) = match pin_side {
                Pin::Right(_) => (vec2(TILE_SIZE / 2., 0.), 0.),
                Pin::Bottom(_) => (vec2(0., TILE_SIZE / 2.), f32::consts::PI / 2.),
                Pin::Left(_) => (vec2(-TILE_SIZE * 2.5, 0.), 0.),
                Pin::Top(_) => (vec2(0., -TILE_SIZE * 2.5), f32::consts::PI / 2.),
            };

            // we can be *reasonably* sure that pin labels won't ever change. so this should be fine. probably.
            state.insert_data(PinLabelMeta(label, text_offset, rotation));
        };
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        let selection = ctx.resource_mut::<PinSelection>();
        if let Some(other) = selection.select(*self) {
            simulation.toggle_connect_by_pinid(other, *self);
        }
    }

    fn render(&self, state: &ObjectContext, simulation: &Simulation, _: &GameObjects) {
        let on_state = simulation.pins.get_state(*self).unwrap();
        let position = state.position();

        let color = if on_state { RED } else { LIGHTGRAY };

        draw_circle(position.x, position.y, TILE_SIZE / 4., color);
        draw_circle_lines(position.x, position.y, TILE_SIZE / 4., 1., BLACK);

        if let Some(meta) = state.get_data::<PinLabelMeta>() {
            let text_pos = position + meta.1;
            let rotation = meta.2;

            draw_text_ex(
                &meta.0,
                text_pos.x,
                text_pos.y,
                TextParams {
                    font_size: 24,
                    rotation,
                    color: BLACK,
                    ..Default::default()
                },
            );
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct InvalidMarker;

impl GameObject for NetworkId {
    fn start(&mut self, state: &mut ObjectContextMut, _: &Simulation) {
        state.set_layer(0);
    }

    fn update(&mut self, state: &mut ObjectContextMut, simulation: &mut Simulation) {
        if simulation.networks.get(*self).is_none() {
            state.despawn();
        }
    }

    fn render(&self, _: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        let network = simulation.networks.get(*self).unwrap();

        let color = if network.state {
            Color::new(0.7, 0.1, 0.1, 0.7)
        } else {
            Color::new(0.1, 0.7, 0.1, 0.7)
        };

        for (a, b) in network.iter_connections() {
            let pos_a = objects.find_state(&a).unwrap().position;
            let pos_b = objects.find_state(&b).unwrap().position;
            draw_line(pos_a.x, pos_a.y, pos_b.x, pos_b.y, 2., color);
        }
    }
}

#[derive(PartialEq)]
struct LedObj(pub ChipId, pub Color);

impl Hash for LedObj {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);

        // reinterpret the 4 f32s in color as one u128, which we can hash.
        let a: u128 = unsafe { std::mem::transmute([self.1.r, self.1.g, self.1.b, self.1.a]) };
        a.hash(state);
    }
}

impl LedObj {
    pub fn new(chip: ChipId, color: Color) -> Self {
        Self(chip, color)
    }
}

impl GameObject for LedObj {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        let instance = simulation.chips.get(self.0).unwrap();

        let (_pos, pin) = instance.pins_as_positions().next().unwrap();

        ctx.spawn_child(pin, ctx.position() - vec2(TILE_SIZE, 0.));
    }

    fn render(&self, state: &ObjectContext, simulation: &Simulation, _: &GameObjects) {
        let chip = simulation.chips.get(self.0).unwrap();
        let pin = chip.get_pinid(Pin::Left(0)).unwrap();

        let Some(network) = simulation.networks.get_network(pin) else {
            return;
        };

        let network_state = simulation.networks.get_state(network).unwrap();

        let color = if network_state.is_high() {
            self.1
        } else {
            Color::from_rgba(255, 255, 255, 100)
        };

        let pos = state.position();

        draw_circle(pos.x, pos.y, TILE_SIZE, color);
        draw_circle_lines(pos.x, pos.y, TILE_SIZE, 1., BLACK);
    }
}

pub struct Camera {
    camera: Camera2D,
    zoom_factor: f32,
}

impl Default for Camera {
    fn default() -> Self {
        let mut camera = Camera2D::default();
        camera.target = vec2(screen_width() / 2., screen_height() / 2.);

        Self {
            camera,
            zoom_factor: 1.5,
        }
    }
}

impl Camera {
    pub fn update(&mut self) {
        if input::is_key_pressed(KeyCode::KpAdd) {
            self.zoom_by(0.1);
        } else if input::is_key_pressed(KeyCode::KpSubtract) {
            self.zoom_by(-0.1);
        }

        let (_, wheel_y) = input::mouse_wheel();
        if wheel_y != 0. {
            self.zoom_by(wheel_y);
        }

        self.camera.zoom = vec2(
            self.zoom_factor / screen_width(),
            self.zoom_factor / screen_height(),
        );

        if input::is_mouse_button_down(MouseButton::Middle) {
            let delta = input::mouse_delta_position();
            self.camera.target += delta * 1000. / self.zoom_factor;
        } else {
            let movement_x = if input::is_key_down(KeyCode::A) {
                -1.
            } else if input::is_key_down(KeyCode::D) {
                1.
            } else {
                0.
            };

            let movement_y = if input::is_key_down(KeyCode::W) {
                -1.
            } else if input::is_key_down(KeyCode::S) {
                1.
            } else {
                0.
            };

            if movement_x != 0. || movement_y != 0. {
                self.camera.target += vec2(movement_x, movement_y).normalize() * 5.;
                // re-set zoom to appropriate level in case window was resized.
            }
        }

        set_camera(&self.camera);
    }

    pub fn get_mouse_world_pos(&self) -> Vec2 {
        let screen_pos = input::mouse_position();
        self.camera
            .screen_to_world(vec2(screen_pos.0, screen_pos.1))
    }

    fn zoom_by(&mut self, by: f32) {
        self.zoom_factor += by;
        if self.zoom_factor <= 0. {
            self.zoom_factor = 0.1;
        }
    }
}

#[derive(Default)]
struct PinSelection {
    other: Option<PinId>,
}

impl PinSelection {
    pub fn select(&mut self, pin: PinId) -> Option<PinId> {
        if self.other.is_none() {
            self.other = Some(pin);
            None
        } else {
            self.other.take()
        }
    }
}
impl GameObject for ChipId {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        ctx.set_layer(1);

        let instance = simulation.chips.get(*self).unwrap();

        ctx.set_shape(Shape::Rectangle(Rect::new(
            ctx.position().x,
            ctx.position().y,
            instance.size.x as f32 * TILE_SIZE,
            instance.size.y as f32 * TILE_SIZE,
        )));

        for (pos, pin) in instance.pins_as_positions() {
            let offset = pos.get_pin_tile_offset(instance.size);

            ctx.spawn_child(pin, ctx.position() + offset);
        }
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, _: &mut Simulation) {
        if !input::is_key_down(KeyCode::LeftAlt) {
            return;
        }
        let mouse_pos = ctx.mouse_world_pos();
        let offset = ChipClickOffset(ctx.position() - mouse_pos);
        ctx.insert_data(offset);
    }

    fn on_click_released(&mut self, ctx: &mut ObjectContextMut, _: &mut Simulation) {
        let snapped = snap_to_grid(ctx.position(), TILE_SIZE);
        ctx.move_by(snapped - ctx.position());
        ctx.delete_data::<ChipClickOffset>();
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, _: &mut Simulation) {
        if let Some(offset) = ctx.get_data::<ChipClickOffset>() {
            let mouse_pos = ctx.mouse_world_pos();
            ctx.move_by(mouse_pos - ctx.position() + offset.0);
        }
    }

    fn render(&self, state: &ObjectContext, simulation: &Simulation, _: &GameObjects) {
        let instance = simulation.chips.get(*self).unwrap();
        let size = instance.size.as_vec2() * TILE_SIZE;
        let position = state.position();

        draw_rectangle(position.x, position.y, size.x, size.y, DARKGRAY);
        draw_rectangle_lines(position.x, position.y, size.x, size.y, 1., BLACK);
    }
}

struct PinLabelMeta(pub String, pub Vec2, pub f32);

#[derive(Default)]
struct ChipClickOffset(pub Vec2);

#[macroquad::main("Chip Game")]
async fn main() {
    request_new_screen_size(1080., 720.);
    //set_fullscreen(true);

    next_frame().await;

    let mut camera = Camera::default();
    camera.update();

    let mut game = Game::default();

    game.resources.insert_default::<PinSelection>();

    game.game_objects.insert(
        Grid {
            axis_color: Color::from_rgba(64, 78, 94, 220),
            axis_width: 2.0,
            ..Grid::new(TILE_SIZE, 200, Color::from_rgba(102, 127, 153, 90))
        },
        vec2(0., 0.),
        &mut game.simulation,
        &mut game.resources,
    );

    let [cpu, high] = game.place_chips(((CPU::default(), ivec2(6, 6)), (TieHigh, ivec2(-2, 6))));

    game.simulation.connect((cpu, "CE"), (high, "HIGH"));

    game.simulation
        .connect((cpu, DATA_PINS[0]), (high, Pin::Right(0)));

    let mut rom = [0; 256];
    for i in 0..u8::MAX {
        rom[i as usize] = i;
    }

    let [rom, high_2, clock_button] = game.place_chips((
        (rom::ROM::from(rom), ivec2(31, 6)),
        (TieHigh, ivec2(22, 5)),
        (button::Button, ivec2(22, 6)),
    ));
    game.simulation.connect((cpu, "CLK"), (clock_button, "OUT"));

    game.simulation.connect((high_2, "HIGH"), (rom, "CE"));

    game.simulation.connect((clock_button, "CLK"), (rom, "CLK"));

    let [display] = game.place_chips((NumericDisplay, ivec2(44, 12)));

    for i in 0..8 {
        game.simulation
            .connect((display, Pin::Top(i)), (rom, Pin::Right(i + 1)));
        game.simulation
            .connect((cpu, Pin::Right(i)), (rom, Pin::Right(i + 1)));
        game.simulation
            .connect((cpu, Pin::Left(i + 4)), (rom, Pin::Right(i + 1)));
    }

    for _ in 0.. {
        clear_background(SKYBLUE);

        game.simulation.tick();

        // sync newly created networks, if any.
        for network in game.simulation.networks.ids().collect::<Vec<_>>() {
            if game.game_objects.find_state(&network).is_none() {
                game.game_objects.insert(
                    network,
                    vec2(0., 0.),
                    &mut game.simulation,
                    &mut game.resources,
                );
            }
        }

        game.update();

        let selection = game.resources.get_mut::<PinSelection>().unwrap();

        if input::is_mouse_button_pressed(MouseButton::Right) {
            selection.other = None;
        }

        let selection = selection.other;

        game.render();

        if let Some(pin) = selection {
            let pin = game.simulation.pins.get(pin).unwrap();
            let label = pin
                .label
                .as_ref()
                .cloned()
                .unwrap_or(format!("Pin {}", pin.id.0));

            draw_text(&format!("Pin selected: {}", label), 0., 100., 32., WHITE);
        }

        draw_fps();

        next_frame().await;
    }
}

struct TieHigh;

impl Chip for TieHigh {
    fn setup(&self) -> PinLayout {
        PinLayout::new_with(
            uvec2(1, 1),
            [(Pin::Right(0), PinDef::new_with_state("HIGH", true))],
        )
    }

    fn update(&mut self, _: &mut PinsState) {}
}

struct Clock {
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
    fn setup(&self) -> PinLayout {
        // _______
        // |      |
        // |      |- CLKB
        // |      |- CLK
        // |______|
        //
        let mut layout = PinLayout::new(1, 2);

        // inverted clock signal
        layout.set(
            Pin::Right(0),
            PinDef {
                label: Some("CLKB".into()),
                initial_state: true,
            },
        );

        // clock signal
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

struct Nand {
    gates: usize,
}

impl Nand {
    pub fn new(gates: usize) -> Self {
        Self { gates }
    }
}

impl Chip for Nand {
    fn setup(&self) -> PinLayout {
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

            let value = !(a && b);
            state.set(Pin::Right(i * 2), value);
        }
    }
}

#[derive(Default)]
struct Counter8b {
    count: u8,
}

impl Chip for Counter8b {
    fn setup(&self) -> PinLayout {
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
        let clock = state.read_wire(Pin::Left(4));

        // for easy chaining. just hook up C7 to CLK on the next chip, and you get a higher bit counter.
        if !clock.is_falling_edge() {
            return;
        }

        self.count = self.count.wrapping_add(1);

        for i in 0..8u8 {
            let pin_state = (self.count & 1u8 << i) > 0;
            state.set(Pin::Right(i as usize), pin_state);
        }
    }
}

struct Led;

impl Chip for Led {
    fn setup(&self) -> PinLayout {
        PinLayout::new_with(uvec2(1, 1), [(Pin::Left(0), PinDef::new("ON"))])
    }

    fn update(&mut self, _: &mut PinsState) {}
}

struct NumericDisplay;

impl Chip for NumericDisplay {
    fn setup(&self) -> PinLayout {
        PinLayout::new_with(
            uvec2(8, 4),
            (0..8).map(|idx| (Pin::Top(idx), PinDef::new(format!("C{idx}")))),
        )
    }

    fn update(&mut self, _: &mut PinsState) {}
}

#[derive(PartialEq, Hash)]
struct NumericDisplayObj(ChipId);

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

    fn update(&mut self, state: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.update(state, simulation);
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
                simulation
                    .networks
                    .get_network(id)
                    .and_then(|network| simulation.networks.get_state(network).map(|n| (index, n)))
            })
            .fold(0_u8, |acc, (index, state)| {
                acc | (state.is_high() as u8) << index
            });

        let text = format!("{}", number);

        let chip_center = ctx.position() + instance.size.as_vec2() * TILE_SIZE / 2.;

        draw_text(
            &text,
            ctx.position().x + TILE_SIZE,
            chip_center.y + 12.,
            56.,
            WHITE,
        );
    }
}
