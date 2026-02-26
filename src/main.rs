use core::f32;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
};

use macroquad::{input, prelude::*};
use petgraph::{graph::NodeIndex, prelude::StableUnGraph};

pub const TILE_SIZE: f32 = 16.0;

#[derive(Default)]
struct GameObjects {
    objects: StableVec<GameObjectData>,
    state: StableVec<GameObjectState>,
}

// TODO: custom metadata on GameObjectState?
// TODO: chips with custom render code. Like LEDs or displays.

/// (barely) copy type storing object metadata.
/// The first 64 bits are the index of the object in the internal vector, the next 64 can be arbitrary metadata,
/// and the last 8 are a type identifier.
/// This metadata is mainly used to find position data about other simulation objects during rendering.
#[derive(Clone, Copy, Debug)]
struct ObjectId(usize, usize, u8);

struct GameObjectData {
    object: Box<dyn GameObject>,
    id: ObjectId,
}

impl GameObjects {
    pub fn find_by_position(&self, point: Vec2) -> Option<ObjectId> {
        for (index, state) in self.state.iter().enumerate() {
            if let Some(shape) = state.shape.as_ref()
                && shape.contains(point)
            {
                return Some(self.objects.buffer[index].as_ref().unwrap().id);
            }
        }

        None
    }

    pub fn insert<O: GameObject + 'static>(
        &mut self,
        mut object: O,
        position: Vec2,
        simulation: &Simulation,
    ) -> ObjectId {
        let mut state = GameObjectState {
            position,
            draw_priority: 0,
            shape: None,
            should_despawn: false,
        };

        object.start(&mut state, simulation, self);

        let mut slot = self.objects.reserve();

        let (meta, marker) = object.make_oid_meta();

        let id = ObjectId(slot.index, meta, marker);

        slot.set(GameObjectData {
            object: Box::new(object),
            id,
        });

        let state_id = self.state.push(state);

        debug_assert_eq!(id.0, state_id);

        id
    }

    /// Use ObjectId metadata taken from [`GameObject::make_oid_meta`] to find the state of the given object.
    /// Mind that if make_oid_data is not implemented (or otherwise returns (0, 0)), this function will never return anything.
    pub fn find_by_meta<O: GameObject>(&self, object: &O) -> Option<&GameObjectState> {
        let (meta, marker) = object.make_oid_meta();
        if meta == 0 && marker == 0 {
            return None;
        }

        let index = self
            .objects
            .iter()
            .find(|o| o.id.1 == meta && o.id.2 == marker)?
            .id
            .0;
        self.state.get(index)
    }

    pub fn update(&mut self, simulation: &mut Simulation) {
        let mut despawned = Vec::new();

        for (object, state) in self.objects.iter_mut().zip(self.state.iter_mut()) {
            object.object.update(state, simulation);
            if state.should_despawn {
                despawned.push(object.id);
            }
        }

        for id in despawned {
            self.objects.remove(id.0);
            self.state.remove(id.0);
        }
    }

    pub fn render(&self, simulation: &Simulation) {
        let mut objects = self
            .objects
            .iter()
            .zip(self.state.iter())
            .collect::<Vec<_>>();

        objects.sort_by(|(_, a), (_, b)| a.draw_priority.cmp(&b.draw_priority));

        for (object, state) in objects.into_iter() {
            object.object.render(state, simulation, &self)
        }
    }
}

enum Shape {
    Rectangle(Rect),
    Circle(Circle),
}

impl Shape {
    pub fn contains(&self, point: Vec2) -> bool {
        match self {
            Self::Rectangle(rect) => rect.contains(point),
            Self::Circle(circle) => circle.contains(&point),
        }
    }
}

struct GameObjectState {
    position: Vec2,
    shape: Option<Shape>,
    /// Objects with higher draw priority are drawn ***later***, thus above objects with a lower priority.
    /// Mind that, within a priority group, there is no guarantee about draw ordering.
    draw_priority: usize,
    should_despawn: bool,
}

impl GameObjectState {
    pub fn despawn(&mut self) {
        self.should_despawn = true;
    }
}

trait GameObject {
    #[allow(unused)]
    fn start(
        &mut self,
        state: &mut GameObjectState,
        simulation: &Simulation,
        objects: &mut GameObjects,
    ) {
    }
    fn render(&self, state: &GameObjectState, simulation: &Simulation, objects: &GameObjects);
    #[allow(unused)]
    fn update(&mut self, state: &mut GameObjectState, simulation: &mut Simulation) {}
    fn make_oid_meta(&self) -> (usize, u8) {
        (0, 0)
    }
}

impl GameObject for ChipId {
    fn start(
        &mut self,
        state: &mut GameObjectState,
        simulation: &Simulation,
        objects: &mut GameObjects,
    ) {
        let instance = simulation.chips.get(*self).unwrap();

        for (pos, pin) in instance.pins_as_positions() {
            let offset = pos.get_pin_tile_offset(instance.size);

            objects.insert(pin, state.position + offset, simulation);
        }
    }

    fn render(&self, state: &GameObjectState, simulation: &Simulation, _: &GameObjects) {
        let instance = simulation.chips.get(*self).unwrap();
        let size = instance.size.as_vec2() * TILE_SIZE;
        let position = state.position;

        draw_rectangle(position.x, position.y, size.x, size.y, DARKGRAY);
        draw_rectangle_lines(position.x, position.y, size.x, size.y, 1., BLACK);
    }

    fn make_oid_meta(&self) -> (usize, u8) {
        (self.0, 1)
    }
}

impl TryFrom<ObjectId> for ChipId {
    type Error = InvalidMarker;
    fn try_from(value: ObjectId) -> Result<Self, Self::Error> {
        let ObjectId(_, id, marker) = value;
        if marker != 1 {
            Err(InvalidMarker)
        } else {
            Ok(Self(id))
        }
    }
}

impl GameObject for PinId {
    fn start(&mut self, state: &mut GameObjectState, _: &Simulation, _: &mut GameObjects) {
        state.draw_priority = 2;

        state.shape = Some(Shape::Circle(Circle {
            x: state.position.x,
            y: state.position.y,
            r: TILE_SIZE / 4.,
        }));
    }

    fn render(&self, state: &GameObjectState, simulation: &Simulation, _: &GameObjects) {
        let on_state = simulation.pins.get_state(*self).unwrap();
        let position = state.position;

        let color = if on_state { RED } else { LIGHTGRAY };

        draw_circle(position.x, position.y, TILE_SIZE / 4., color);
        draw_circle_lines(position.x, position.y, TILE_SIZE / 4., 1., BLACK);

        let pin = simulation.pins.get(*self).unwrap();

        // TODO: text is very unreadable right now. It should probably be offset in the same direction the pin is relative to the chip.
        if let Some(ref text) = pin.label {
            // FIXME: this is really inefficient, since we need to traverse every chip's pin every time.
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

            let text_pos = position + text_offset;

            draw_text_ex(
                text,
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

    fn make_oid_meta(&self) -> (usize, u8) {
        (self.0, 2)
    }
}

#[derive(Clone, Copy, Debug)]
struct InvalidMarker;

impl TryFrom<ObjectId> for PinId {
    type Error = InvalidMarker;
    fn try_from(value: ObjectId) -> Result<Self, Self::Error> {
        let ObjectId(_, id, marker) = value;
        if marker != 2 {
            Err(InvalidMarker)
        } else {
            Ok(Self(id))
        }
    }
}

impl GameObject for NetworkId {
    fn start(&mut self, state: &mut GameObjectState, _: &Simulation, _: &mut GameObjects) {
        state.draw_priority = 1;
    }

    fn update(&mut self, state: &mut GameObjectState, simulation: &mut Simulation) {
        if simulation.networks.get(*self).is_none() {
            state.despawn();
        }
    }

    fn render(&self, _: &GameObjectState, simulation: &Simulation, objects: &GameObjects) {
        let network = simulation.networks.get(*self).unwrap();

        let color = if network.state { RED } else { GREEN };

        for (a, b) in network.iter_connections() {
            let pos_a = objects.find_by_meta(&a).unwrap().position;
            let pos_b = objects.find_by_meta(&b).unwrap().position;
            draw_line(pos_a.x, pos_a.y, pos_b.x, pos_b.y, 2., color);
        }
    }

    fn make_oid_meta(&self) -> (usize, u8) {
        (self.0, 3)
    }
}

impl TryFrom<ObjectId> for NetworkId {
    type Error = InvalidMarker;
    fn try_from(value: ObjectId) -> Result<Self, Self::Error> {
        let ObjectId(_, id, marker) = value;
        if marker != 3 {
            Err(InvalidMarker)
        } else {
            Ok(Self(id))
        }
    }
}

struct Camera {
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
    fn update(&mut self) {
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

        set_camera(&self.camera);
    }

    fn zoom_by(&mut self, by: f32) {
        self.zoom_factor += by;
        if self.zoom_factor <= 0. {
            self.zoom_factor = 0.1;
        }
    }
}

#[macroquad::main("Chip Game")]
async fn main() {
    request_new_screen_size(1080., 720.);
    //set_fullscreen(true);

    next_frame().await;

    let mut camera = Camera::default();
    camera.update();

    let mut simulation = Simulation::default();
    let mut gameobjects = GameObjects::default();

    // let random = simulation.place_chip(RandomNxN::new(16));
    // gameobjects.insert(random, vec2(100., 100.), &simulation);

    // let nand = simulation.place_chip(Nand::new(3));
    // gameobjects.insert(nand, vec2(600., 100.), &simulation);

    // let counter = simulation.place_chip(Counter8b::default());
    // gameobjects.insert(counter, vec2(600., 400.), &simulation);

    // for i in 0..3 {
    //     simulation.connect(random, Pin::Right(i), random, Pin::Right(i + 1));
    //     simulation.connect(nand, Pin::Left(i), nand, Pin::Left(i + 1));
    // }

    // simulation
    //     .connect(random, Pin::Right(0), nand, Pin::Left(0))
    //     .unwrap();
    let clock = simulation.place_chip(Clock::new(5));
    gameobjects.insert(clock, vec2(100., 100.), &simulation);

    let counter = simulation.place_chip(Counter8b::default());
    gameobjects.insert(counter, vec2(300., 100.), &simulation);

    simulation.connect(clock, Pin::Right(1), counter, Pin::Left(4));

    let counter2 = simulation.place_chip(Counter8b::default());
    gameobjects.insert(counter2, vec2(300., 250.), &simulation);

    simulation.connect(counter, Pin::Right(7), counter2, Pin::Left(4));

    let mut selected_pin: Option<PinId> = None;

    for _ in 0.. {
        clear_background(SKYBLUE);

        simulation.tick();

        if input::is_mouse_button_pressed(MouseButton::Left) {
            let world_pos = camera
                .camera
                .screen_to_world(input::mouse_position().into());

            if let Some(object) = gameobjects.find_by_position(world_pos)
                && let Ok(pin) = PinId::try_from(object)
            {
                if let Some(first_pin) = selected_pin {
                    simulation.networks.toggle_connect(first_pin, pin);
                    selected_pin = None;
                } else {
                    selected_pin = Some(pin);
                }
            }
        } else if input::is_mouse_button_pressed(MouseButton::Right) {
            selected_pin = None;
        }

        if let Some(pin) = selected_pin {
            let pin = simulation.pins.get(pin).unwrap();
            let label = pin
                .label
                .as_ref()
                .cloned()
                .unwrap_or(format!("Pin {}", pin.id.0));

            draw_text(&format!("Pin selected: {}", label), 0., 100., 32., WHITE);
        }

        // sync newly created networks, if any.
        for network in simulation.networks.ids() {
            if gameobjects.find_by_meta(&network).is_none() {
                gameobjects.insert(network, vec2(0., 0.), &simulation);
            }
        }

        camera.update();

        gameobjects.update(&mut simulation);
        gameobjects.render(&simulation);

        draw_fps();

        next_frame().await;
    }
}

struct Test {
    current_tick: u8,
    pin: Pin,
}

struct RandomNxN {
    n: usize,
}

impl RandomNxN {
    pub fn new(n: usize) -> Self {
        Self { n }
    }
}

impl TryFrom<(usize, usize)> for Pin {
    type Error = ();
    fn try_from((side, id): (usize, usize)) -> Result<Self, Self::Error> {
        Ok(match side {
            0 => Pin::Right(id),
            1 => Pin::Bottom(id),
            2 => Pin::Left(id),
            3 => Pin::Top(id),
            _ => return Err(()),
        })
    }
}

impl Chip for RandomNxN {
    fn setup(&self) -> PinLayout {
        PinLayout::new_with(
            uvec2(self.n as u32, self.n as u32),
            (0..4)
                .flat_map(|side| std::iter::repeat(side).zip(0..self.n as usize))
                .map(|(side, id)| {
                    let pin = match side {
                        0 => Pin::Right(id),
                        1 => Pin::Bottom(id),
                        2 => Pin::Left(id),
                        3 => Pin::Top(id),
                        _ => unreachable!(),
                    };

                    (pin, PinDef::new(simple_pin_name(pin)))
                }),
        )
    }

    fn update(&mut self, state: &mut PinsState) {
        let side = rand::gen_range::<usize>(0, 4);
        let id = rand::gen_range::<usize>(0, self.n);

        let pin = Pin::try_from((side, id)).unwrap();
        state.toggle(pin);
    }
}

impl Test {
    pub fn new(pin: Pin) -> Self {
        Self {
            current_tick: 0,
            pin,
        }
    }
}

fn simple_pin_name(pin: Pin) -> String {
    match pin {
        Pin::Right(i) => format!("R{i}"),
        Pin::Bottom(i) => format!("B{i}"),
        Pin::Left(i) => format!("L{i}"),
        Pin::Top(i) => format!("T{i}"),
    }
}

impl Chip for Test {
    fn setup(&self) -> PinLayout {
        PinLayout::new_with(
            uvec2(2, 2),
            [(self.pin, PinDef::new(simple_pin_name(self.pin)))],
        )
    }

    fn update(&mut self, state: &mut PinsState) {
        self.current_tick += 1;
        self.current_tick %= 8;

        let id = self.current_tick as usize % 2;

        let pin = match self.current_tick {
            0 | 1 => Pin::Right(id),
            2 | 3 => Pin::Bottom(id),
            4 | 5 => Pin::Left(id),
            6 | 7 => Pin::Top(id),
            _ => unreachable!(),
        };

        state.try_toggle(pin);
    }
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

        if self.current_tick % self.interval == 0 {
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
            let a = state
                .read_wire(Pin::Left(2 * i))
                .unwrap_or_default()
                .is_high();
            let b = state
                .read_wire(Pin::Left(2 * i + 1))
                .unwrap_or_default()
                .is_high();

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
        let clock = state.read_wire(Pin::Left(4)).unwrap_or_default();

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

#[derive(Default)]
struct Simulation {
    chips: Chips,
    pins: Pins,
    networks: Networks,
}

impl Simulation {
    pub fn place_chip<T: Chip + 'static>(&mut self, chip: T) -> ChipId {
        let id = self.chips.register(&mut self.pins, chip);
        id
    }

    pub fn tick(&mut self) {
        for chip in self.chips.chips.iter_mut() {
            let mut state = PinsState {
                pin_ids: &chip.pins,
                pin_size: chip.size,
                states: &mut self.pins,
                networks: &self.networks,
                mutations: Vec::new(),
            };
            chip.chip.update(&mut state);

            state.apply();
        }

        self.networks.update(&self.pins);
    }

    pub fn connect(
        &mut self,
        chip_a: ChipId,
        pin_a: Pin,
        chip_b: ChipId,
        pin_b: Pin,
    ) -> Option<()> {
        let chip_a = self.chips.get(chip_a)?;
        let pin_a = chip_a.get_pinid(pin_a)?;

        let chip_b = self.chips.get(chip_b)?;
        let pin_b = chip_b.get_pinid(pin_b)?;

        self.networks.toggle_connect(pin_a, pin_b);
        Some(())
    }

    pub fn remove_chip(&mut self, chip: ChipId) {
        let chip = self.chips.chips.remove(chip.0).unwrap();
        for pin in chip.pins.into_iter().filter_map(|p| p) {
            self.pins.pins.remove(pin.0);
            self.networks.remove_pin(pin);
        }
    }
}

#[derive(Default)]
struct Chips {
    chips: StableVec<ChipInstance>,
}

impl Chips {
    pub fn register<T: Chip + 'static>(&mut self, pins: &mut Pins, chip: T) -> ChipId {
        let mut slot = self.chips.reserve();

        let id = ChipId(slot.index);
        let pin_layout = chip.setup();

        let pin_ids = pins.register_all(id, &pin_layout);

        slot.set(ChipInstance {
            id,
            chip: Box::new(chip),
            size: pin_layout.size,
            pins: pin_ids,
        });

        id
    }

    pub fn get(&self, chip: ChipId) -> Option<&ChipInstance> {
        self.chips.buffer.get(chip.0)?.as_ref()
    }
}

pub trait Chip {
    fn setup(&self) -> PinLayout;
    fn update(&mut self, state: &mut PinsState);
}

pub struct ChipInstance {
    chip: Box<dyn Chip>,
    /// Pins are stored clockwise, from Right(0) to Top(size.x).
    pins: Vec<Option<PinId>>,
    size: UVec2,
    id: ChipId,
}

impl ChipInstance {
    fn pins_as_positions(&self) -> impl Iterator<Item = (Pin, PinId)> {
        self.pins
            .iter()
            .enumerate()
            .filter_map(|(index, p)| p.map(|pin| (Pin::from_index(index, self.size), pin)))
    }

    pub fn get_pinid(&self, pin: Pin) -> Option<PinId> {
        let index = pin.as_pinid_index(self.size);
        *self.pins.get(index)?
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChipId(usize);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct PinId(usize);
pub struct PinMeta {
    chip: ChipId,
    label: Option<String>,
    id: PinId,
    state: bool,
}

#[derive(Default)]
pub struct Pins {
    pins: StableVec<PinMeta>,
}

impl Pins {
    pub fn register(&mut self, chip: ChipId, pin: PinDef) -> PinId {
        let mut slot = self.pins.reserve();
        let id = PinId(slot.index);

        let PinDef {
            label,
            initial_state,
        } = pin;

        slot.set(PinMeta {
            chip,
            id,
            label,
            state: initial_state,
        });

        PinId(slot.index)
    }

    pub fn register_all(&mut self, id: ChipId, layout: &PinLayout) -> Vec<Option<PinId>> {
        let mut pins = Vec::new();

        for pin_def in layout.state.iter() {
            let id = pin_def.clone().map(|def| self.register(id, def));
            pins.push(id);
        }

        println!("Pins for {id:?}: {:?}", pins);

        pins
    }

    pub fn get_state(&self, pin: PinId) -> Option<bool> {
        self.pins
            .buffer
            .get(pin.0)
            .and_then(|p| p.as_ref())
            .map(|p| p.state)
    }

    pub fn get(&self, pin: PinId) -> Option<&PinMeta> {
        self.pins.get(pin.0)
    }
}

enum PinMutation {
    Toggle,
    Set(bool),
}

pub struct PinsState<'a> {
    /// Size of the pin layout (for figuring out which Pin location corresponds to which PinId)
    pin_size: UVec2,
    pin_ids: &'a [Option<PinId>],
    states: &'a mut Pins,
    networks: &'a Networks,
    // mutations are buffered. not even sure we need that, but hey.
    mutations: Vec<(PinId, PinMutation)>,
}

impl PinsState<'_> {
    // translates a PinLocation into a PinId for this state.
    fn get_pin_id(&self, location: Pin) -> Option<PinId> {
        let index = location.as_pinid_index(self.pin_size);

        self.pin_ids.get(index)?.as_ref().copied()
    }

    pub fn try_toggle(&mut self, pin: Pin) -> Option<()> {
        let pin = self.get_pin_id(pin)?;
        self.mutations.push((pin, PinMutation::Toggle));
        Some(())
    }

    pub fn toggle(&mut self, pin: Pin) -> &mut Self {
        let pin = self.get_pin_id(pin).unwrap();
        self.mutations.push((pin, PinMutation::Toggle));
        self
    }

    pub fn on(&mut self, pin: Pin) -> &mut Self {
        let pin = self.get_pin_id(pin).unwrap();
        self.mutations.push((pin, PinMutation::Set(true)));
        self
    }

    pub fn off(&mut self, pin: Pin) -> &mut Self {
        let pin = self.get_pin_id(pin).unwrap();
        self.mutations.push((pin, PinMutation::Set(false)));
        self
    }

    pub fn set(&mut self, pin: Pin, state: bool) -> &mut Self {
        let pin = self.get_pin_id(pin).unwrap();
        self.mutations.push((pin, PinMutation::Set(state)));
        self
    }

    fn apply(self) {
        for (pin, mutation) in self.mutations {
            let pin = self.states.pins.get_mut(pin.0).unwrap();
            let new_state = match mutation {
                PinMutation::Toggle => !pin.state,
                PinMutation::Set(state) => state,
            };
            pin.state = new_state;
        }
    }

    /// Reads the current *output* of this pin. This is almost never what you need.
    /// See [`Self::read_wire`] instead to read the input.
    /// If this is false, but [`Self::read_wire`] is true, that means some other pin connected to this one is high.
    pub fn read_output(&self, pin: Pin) -> Option<bool> {
        let id = self.get_pin_id(pin)?;
        let state = self.states.pins.buffer.get(id.0)?.as_ref();
        state.map(|s| s.state)
    }

    /// Reads the current input state of this pin, usually provided by other chips.
    pub fn read_wire(&self, pin: Pin) -> Option<NetworkState> {
        let pin = self.get_pin_id(pin)?;
        let network = self.networks.get_network(pin)?;
        self.networks.get_state(network)
    }
}

#[derive(Default, Clone, Debug)]
pub struct PinDef {
    label: Option<String>,
    initial_state: bool,
}

impl PinDef {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: Some(label.into()),
            initial_state: false,
        }
    }

    pub fn new_with_state(label: impl Into<String>, initial_state: bool) -> Self {
        Self {
            label: Some(label.into()),
            initial_state,
        }
    }
}

#[derive(Debug)]
pub struct PinLayout {
    size: UVec2,
    state: Vec<Option<PinDef>>,
}

impl PinLayout {
    pub fn new(x: usize, y: usize) -> Self {
        let state = vec![None; 2 * x as usize + 2 * y as usize];

        Self {
            size: uvec2(x as u32, y as u32),
            state,
        }
    }

    pub fn new_with(size: UVec2, pins: impl IntoIterator<Item = (Pin, PinDef)>) -> Self {
        let mut layout = Self::new(size.x as usize, size.y as usize);

        for (pin, def) in pins.into_iter() {
            layout.set(pin, def);
        }

        layout
    }

    pub fn set(&mut self, location: Pin, pin: PinDef) {
        let index = location.as_pinid_index(self.size);
        self.state[index] = Some(pin);
    }

    pub fn delete(&mut self, location: Pin) {
        let index = location.as_pinid_index(self.size);
        self.state[index] = None;
    }
}

/// Describes the location of a pin on the edge of a chip. Pins are counted left to right, top to bottom, and cannot be on corners.
/// So Pin::Top(0) is leftmost pin on the top side of the chip.
#[derive(Clone, Copy, Debug)]
pub enum Pin {
    Top(usize),
    Right(usize),
    Bottom(usize),
    Left(usize),
}

impl Pin {
    fn inner_index(&self) -> usize {
        match self {
            Self::Top(u) | Self::Right(u) | Self::Bottom(u) | Self::Left(u) => *u,
        }
    }

    // TODO: `as_layout_index` is redundant now, so remove and rewrite as_pinid_index

    /// As an index into a flat PinId Vec/slice, usually on `ChipInstance`.
    fn as_pinid_index(&self, size: UVec2) -> usize {
        let (outer, inner) = self.as_layout_index();
        let offset = match outer {
            0 => 0,                                     // right
            1 => size.x as usize,                       // bottom
            2 => size.x as usize + size.y as usize,     // left
            3 => size.x as usize * 2 + size.y as usize, // top
            _ => unreachable!(),
        };

        offset + inner
    }

    fn as_layout_index(&self) -> (usize, usize) {
        let inner = self.inner_index();
        let outer = match self {
            Self::Right(_) => 0,
            Self::Bottom(_) => 1,
            Self::Left(_) => 2,
            Self::Top(_) => 3,
        };

        (outer, inner)
    }

    fn from_index(index: usize, size: UVec2) -> Self {
        let mut offset = index;

        let mut side = 0;

        for (side_index, segment_size) in [
            size.y as usize, // right
            size.x as usize, // bottom
            size.y as usize, // left
            size.x as usize, // top
        ]
        .into_iter()
        .enumerate()
        {
            side = side_index;
            if let Some(new_offset) = offset.checked_sub(segment_size) {
                offset = new_offset;
            } else {
                break;
            }
        }

        match side {
            0 => Self::Right(offset),
            1 => Self::Bottom(size.x as usize - offset),
            2 => Self::Left(size.y as usize - offset),
            3 => Self::Top(offset),
            _ => unreachable!(),
        }
    }

    fn get_pin_tile_offset(&self, size: UVec2) -> Vec2 {
        let offset = TILE_SIZE / 2.0;

        match self {
            Self::Top(idx) => vec2(*idx as f32 * TILE_SIZE + offset, 0.),
            Self::Right(idx) => vec2(size.x as f32 * TILE_SIZE, *idx as f32 * TILE_SIZE + offset),
            Self::Bottom(idx) => vec2(
                (size.x as usize - *idx) as f32 * TILE_SIZE + offset,
                size.y as f32 * TILE_SIZE,
            ),
            Self::Left(idx) => vec2(0., (size.y as usize - *idx) as f32 * TILE_SIZE + offset),
        }
    }
}

/// A vector where indices for elements remain stable across removals.
/// Mind that this implementation does *not* guarantee that pointers to elements in the vector will remain stable, only the indices.
struct StableVec<T> {
    buffer: Vec<Option<T>>,
}

impl<T> Default for StableVec<T> {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
        }
    }
}

struct Slot<'a, T> {
    index: usize,
    slot: &'a mut Option<T>,
}

impl<T> Slot<'_, T> {
    pub fn set(&mut self, value: T) {
        *self.slot = Some(value);
    }

    pub fn delete(&mut self) -> Option<T> {
        self.slot.take()
    }
}

impl<T> StableVec<T> {
    pub fn slots(&self) -> usize {
        self.buffer.len()
    }

    pub fn insert_with<F: FnOnce(usize) -> T>(&mut self, f: F) -> usize {
        let mut slot = self.reserve();
        slot.set(f(slot.index));
        slot.index
    }

    pub fn reserve(&mut self) -> Slot<'_, T> {
        let index = self
            .buffer
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
            .map(|(idx, _)| idx)
            .unwrap_or_else(|| {
                self.buffer.push(None);
                self.buffer.len() - 1
            });

        let slot = &mut self.buffer[index];
        Slot { index, slot }
    }

    pub fn push(&mut self, element: T) -> usize {
        let mut slot = self.reserve();
        slot.set(element);
        slot.index
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        if let Some(slot) = self.buffer.get_mut(index) {
            slot.take()
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.buffer.get_mut(index)?.as_mut()
    }

    pub fn get(&self, index: usize) -> Option<&T> {
        self.buffer.get(index)?.as_ref()
    }

    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.buffer
            .iter()
            .filter(|slot| slot.is_some())
            .map(|slot| slot.as_ref().unwrap())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.buffer
            .iter_mut()
            .filter(|slot| slot.is_some())
            .map(|slot| slot.as_mut().unwrap())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct NetworkId(usize);

struct Network {
    pins: NetworkPins,
    state: bool,
    last_state: bool,
    id: NetworkId,
}

#[derive(Clone, Default)]
struct NetworkPins {
    pins: HashMap<PinId, NodeIndex<usize>>,
    connections: StableUnGraph<PinId, (), usize>,
}

impl NetworkPins {
    fn get_or_insert_node_idx(&mut self, pin: PinId) -> NodeIndex<usize> {
        if let Some(idx) = self.pins.get(&pin) {
            *idx
        } else {
            let id = self.connections.add_node(pin);
            self.pins.insert(pin, id);
            id
        }
    }

    /// Returns true if an edge was added to the graph, and false if it was not.
    pub fn connect(&mut self, pin_a: PinId, pin_b: PinId) -> bool {
        let idx_a = self.get_or_insert_node_idx(pin_a);
        let idx_b = self.get_or_insert_node_idx(pin_b);

        if self.connections.contains_edge(idx_a, idx_b) {
            false
        } else {
            self.connections.add_edge(idx_a, idx_b, ());
            true
        }
    }

    pub fn iter_connections(&self) -> impl Iterator<Item = (PinId, PinId)> {
        self.connections
            .edge_indices()
            .filter_map(|index| self.connections.edge_endpoints(index))
            .map(|(a, b)| {
                (
                    self.connections.node_weight(a).copied().unwrap(),
                    self.connections.node_weight(b).copied().unwrap(),
                )
            })
    }

    fn reprocess_graph(&mut self) -> Option<GraphMutationResult> {
        let mut graphs = find_isolated_subgraphs(&self.connections);
        // we sort reverse by size and skip one as we want to be the largest new graph.
        graphs.sort_by(|l, r| r.len().cmp(&l.len()));

        if graphs[0].len() <= 1 {
            // this is the largest networks, and even it is too small to exist now.
            // therefore, none of the (sub-)networks can now exist.
            return Some(GraphMutationResult::NetworkRemovalRequired);
        }

        let mut new_data = Vec::new();

        for graph in graphs.into_iter().skip(1) {
            if graph.len() == 1 {
                let index = graph.into_iter().next().unwrap();
                if let Some(pin) = self.connections.remove_node(index) {
                    self.pins.remove(&pin);
                }
                continue;
            }

            let mut data = NetworkPins::default();

            for (a, b) in graph.into_iter().flat_map(|node| {
                self.connections
                    .neighbors(node)
                    .zip(std::iter::repeat(node))
                    .map(|(a, b)| (self.connections[a], self.connections[b]))
            }) {
                data.connect(a, b);
            }

            for pin in data.pins.keys() {
                let idx = self.pins.remove(pin).unwrap();
                self.connections.remove_node(idx);
            }

            new_data.push(data);
        }

        if new_data.len() > 0 {
            Some(GraphMutationResult::CreateNetworks(new_data))
        } else {
            None
        }
    }

    pub fn remove(&mut self, pin: PinId) -> Option<GraphMutationResult> {
        let idx = self.pins.remove(&pin)?;
        self.connections.remove_node(idx);

        self.reprocess_graph()
    }

    pub fn disconnect(&mut self, pin_a: PinId, pin_b: PinId) -> Option<GraphMutationResult> {
        let a = self.pins.get(&pin_a)?;
        let b = self.pins.get(&pin_b)?;

        let edge = self.connections.find_edge(*a, *b)?;

        self.connections.remove_edge(edge).unwrap();

        self.reprocess_graph()
    }
}

impl Network {
    pub fn get_or_insert_node_idx(&mut self, pin: PinId) -> NodeIndex<usize> {
        self.pins.get_or_insert_node_idx(pin)
    }

    /// Returns true if a new connection was added to the network, false if the connection already existed.
    pub fn connect(&mut self, pin_a: PinId, pin_b: PinId) -> bool {
        self.pins.connect(pin_a, pin_b)
    }

    pub fn iter_connections(&self) -> impl Iterator<Item = (PinId, PinId)> {
        self.pins.iter_connections()
    }

    pub fn disconnect(&mut self, pin_a: PinId, pin_b: PinId) -> Option<GraphMutationResult> {
        self.pins.disconnect(pin_a, pin_b)
    }

    /// Remove a pin from the network. If the removal of the pin resulted in one or more disconnected sub-graphs,
    /// returns the data to create a new network from the removed pins. This network will always remain as the largest part of the splintered network.
    /// If singular nodes get disconnected, they are automatically removed from the network.
    pub fn remove(&mut self, pin: PinId) -> Option<GraphMutationResult> {
        self.pins.remove(pin)
    }
}

fn find_isolated_subgraphs<N, W>(graph: &StableUnGraph<N, W, usize>) -> Vec<Vec<NodeIndex<usize>>> {
    let mut sub_graphs = Vec::new();

    let mut to_visit = graph.node_indices().collect::<Vec<_>>();
    let mut visited = HashSet::new();

    while let Some(node) = to_visit.pop() {
        if visited.contains(&node) {
            continue;
        }

        visited.insert(node);

        let mut nodes = HashSet::from([node]);

        fn flood_fill<N, W>(
            graph: &StableUnGraph<N, W, usize>,
            node: NodeIndex<usize>,
            nodes: &mut HashSet<NodeIndex<usize>>,
        ) {
            for neighbor in graph.neighbors(node) {
                if !nodes.contains(&neighbor) {
                    nodes.insert(neighbor);
                    flood_fill(graph, neighbor, nodes);
                }
            }
        }

        flood_fill(graph, node, &mut nodes);

        visited.extend(&nodes);

        sub_graphs.push(nodes.into_iter().collect());
    }

    sub_graphs
}

enum GraphMutationResult {
    /// Returned from [`Network::remove`] if the removal resulted in the network becoming too small to exist, so a removal of the network is required.
    NetworkRemovalRequired,
    /// Returned from [`Network::remove`] if the removal resulted in one or more isolated sub networks.
    CreateNetworks(Vec<NetworkPins>),
}

impl Network {
    pub fn new(id: NetworkId) -> Self {
        Self {
            id,
            pins: Default::default(),
            state: false,
            last_state: false,
        }
    }

    pub fn update(&mut self, pins: &Pins) {
        self.last_state = self.state;
        self.state = self
            .pins
            .connections
            .node_weights()
            .any(|p| pins.get_state(*p).unwrap_or(false));
    }
}

#[derive(Default)]
struct Networks {
    networks: StableVec<Network>,
}

#[derive(Clone, Copy, Debug, Default)]
pub enum NetworkState {
    /// Network is transitioning from low to high. This is counted as a high state for [Self::is_high].
    RisingEdge,
    /// Network is transitioning from high to low. This is counted as a low state for [Self::is_low].
    FallingEdge,
    /// Network is stable high
    High,
    #[default]
    /// Network is stable low
    Low,
}

impl NetworkState {
    pub fn is_rising_edge(&self) -> bool {
        matches!(self, Self::RisingEdge)
    }

    pub fn is_falling_edge(&self) -> bool {
        matches!(self, Self::FallingEdge)
    }

    pub fn is_high(&self) -> bool {
        matches!(self, Self::High | Self::RisingEdge)
    }

    pub fn is_low(&self) -> bool {
        matches!(self, Self::Low | Self::FallingEdge)
    }

    pub fn new(last_state: bool, current_state: bool) -> Self {
        match (last_state, current_state) {
            (true, true) => Self::High,
            (true, false) => Self::FallingEdge,
            (false, false) => Self::Low,
            (false, true) => Self::RisingEdge,
        }
    }
}

impl Networks {
    pub fn update(&mut self, pins: &Pins) {
        for network in self.networks.iter_mut() {
            network.update(pins);
        }
    }

    pub fn get(&self, network: NetworkId) -> Option<&Network> {
        self.networks.get(network.0)
    }

    pub fn ids(&self) -> impl Iterator<Item = NetworkId> {
        self.networks
            .buffer
            .iter()
            .enumerate()
            .filter_map(|(id, network)| network.as_ref().map(|_| NetworkId(id)))
    }

    pub fn get_state(&self, network: NetworkId) -> Option<NetworkState> {
        self.networks
            .buffer
            .get(network.0)?
            .as_ref()
            .map(|n| NetworkState::new(n.last_state, n.state))
    }

    pub fn get_network(&self, pin: PinId) -> Option<NetworkId> {
        for network in self.networks.iter() {
            if network.pins.pins.contains_key(&pin) {
                return Some(network.id);
            }
        }
        None
    }

    pub fn get_or_create_network(&mut self, pin: PinId) -> NetworkId {
        if let Some(network) = self.get_network(pin) {
            return network;
        }

        let mut slot = self.networks.reserve();
        let id = NetworkId(slot.index);
        slot.set(Network::new(id));

        id
    }

    fn merge(&mut self, network_a: NetworkId, network_b: NetworkId) {
        let move_into;
        let move_from;

        match network_a.0.cmp(&network_b.0) {
            // trying to merge one network into itself is a noop.
            Ordering::Equal => return,
            Ordering::Less => {
                move_into = network_a;
                move_from = network_b;
            }
            Ordering::Greater => {
                move_into = network_b;
                move_from = network_a;
            }
        }

        let move_from = self.networks.remove(move_from.0).unwrap();
        let move_into = self.networks.get_mut(move_into.0).unwrap();

        for (pin_a, pin_b) in move_from.iter_connections() {
            move_into.connect(pin_a, pin_b);
        }
    }

    fn handle_mutation(&mut self, network_id: NetworkId, mutation: Option<GraphMutationResult>) {
        match mutation {
            None => {}
            Some(GraphMutationResult::CreateNetworks(networks)) => {
                for pins in networks {
                    self.networks.insert_with(|id| Network {
                        id: NetworkId(id),
                        pins,
                        state: false,
                        last_state: false,
                    });
                }
            }
            Some(GraphMutationResult::NetworkRemovalRequired) => {
                self.networks.remove(network_id.0);
            }
        }
    }

    pub fn toggle_connect(&mut self, pin_a: PinId, pin_b: PinId) {
        let network_a = self.get_network(pin_a);
        let network_b = self.get_network(pin_b);

        match (network_a, network_b) {
            // Neither pin is in a network; create a new one
            (None, None) => {
                let mut slot = self.networks.reserve();
                let id = NetworkId(slot.index);

                let mut network = Network::new(id);
                network.connect(pin_a, pin_b);

                slot.set(network);
            }
            // both pins are in the same network; just connect them.
            (Some(a), Some(b)) if a == b => {
                let network = self.networks.get_mut(a.0).unwrap();
                if !network.connect(pin_a, pin_b) {
                    let mutation = network.disconnect(pin_a, pin_b);
                    self.handle_mutation(a, mutation);
                }
            }
            // pins are in different networks; merge them
            (Some(a), Some(b)) => self.merge(a, b),
            // only one pin is in a network - add the other to it.
            (Some(network), None) | (None, Some(network)) => {
                let network = self.networks.get_mut(network.0).unwrap();
                network.connect(pin_a, pin_b);
            }
        }
    }

    pub fn remove_pin(&mut self, pin: PinId) {
        let Some(network_id) = self.get_network(pin) else {
            return;
        };

        let network = self.networks.get_mut(network_id.0).unwrap();

        let mutation = network.remove(pin);

        self.handle_mutation(network_id, mutation);
    }
}

#[cfg(test)]
mod tests {
    use petgraph::prelude::StableUnGraph;

    use crate::{Nand, NetworkId, Pin, Simulation, find_isolated_subgraphs};

    #[test]
    fn test_merging() {
        let mut simulation = Simulation::default();

        let a = simulation.place_chip(Nand::new(1));
        let b = simulation.place_chip(Nand::new(1));

        simulation.connect(a, Pin::Left(0), a, Pin::Left(1));
        simulation.connect(b, Pin::Left(0), b, Pin::Left(1));

        simulation.connect(a, Pin::Left(0), b, Pin::Left(1));

        assert_eq!(simulation.networks.networks.iter().count(), 1);

        let network = simulation.networks.get(NetworkId(0)).unwrap();

        assert_eq!(network.pins.pins.len(), 4);
    }

    #[test]
    fn test_isolated_subgraphs() {
        let mut graph = StableUnGraph::<(), (), usize>::with_capacity(0, 0);
        let a = graph.add_node(());
        let b = graph.add_node(());
        graph.add_edge(a, b, ());

        let c = graph.add_node(());
        let d = graph.add_node(());
        graph.add_edge(c, d, ());

        let graphs = find_isolated_subgraphs(&graph);

        assert_eq!(graphs.len(), 2, "wrong amount of subgraphs");
        assert_eq!(graphs[0].len(), 2, "wrong subgraph length 1");
        assert_eq!(graphs[1].len(), 2, "wrong subgraph length 2");
    }
}
