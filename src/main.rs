use std::{cmp::Ordering, collections::HashSet};

use macroquad::{input, prelude::*};

pub const TILE_SIZE: f32 = 16.0;

#[derive(Default)]
struct GameObjects {
    objects: StableVec<GameObjectData>,
    state: StableVec<GameObjectState>,
}

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
        state.draw_priority = 1;

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

        let pin = simulation.pins.get(*self).unwrap();

        if let Some(ref text) = pin.label {
            draw_text(text, position.x, position.y - TILE_SIZE / 2., 24., BLACK);
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
    fn update(&mut self, state: &mut GameObjectState, simulation: &mut Simulation) {
        if simulation.networks.get(*self).is_none() {
            state.despawn();
        }
    }

    fn render(&self, _: &GameObjectState, simulation: &Simulation, objects: &GameObjects) {
        let network = simulation.networks.get(*self).unwrap();

        let mut targets = Vec::with_capacity(network.pins.len());

        for pin in network.pins.iter() {
            let pin_state = objects.find_by_meta(pin).unwrap();
            targets.push(pin_state.position);
        }

        let color = if network.state { RED } else { GREEN };

        for window in targets.windows(2) {
            let a = window[0];
            let b = window[1];
            draw_line(a.x, a.y, b.x, b.y, 5., color);
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
    }
}

#[macroquad::main("Test")]
async fn main() {
    request_new_screen_size(1080., 720.);
    set_fullscreen(true);

    next_frame().await;

    let mut camera = Camera::default();
    camera.update();

    let mut simulation = Simulation::default();
    let mut gameobjects = GameObjects::default();

    let random = simulation.place_chip(RandomNxN::new(16));
    gameobjects.insert(random, vec2(100., 100.), &simulation);

    let nand = simulation.place_chip(Nand::new(3));
    gameobjects.insert(nand, vec2(600., 100.), &simulation);

    for i in 0..6 {
        simulation.connect(random, Pin::Right(i), nand, Pin::Left(i));
    }

    for network_id in simulation.networks.ids() {
        gameobjects.insert(network_id, Vec2::ZERO, &simulation);
    }

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
                    simulation.networks.connect(first_pin, pin);
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
        let mut layout = PinLayout::new(1, 1);

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
            println!("Making gate {i} for Nand.");
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
            let a = state.read_wire(Pin::Left(2 * i)).unwrap();
            let b = state.read_wire(Pin::Left(2 * i + 1)).unwrap();

            let value = !(a && b);

            println!("Setting Nand to {value}");

            state.set(Pin::Right(i * 2), value);
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

        self.networks.connect(pin_a, pin_b);
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
            if let Some(pin) = pin_def.as_ref() {
                println!("Registering pin {:?} of {:?}", pin, id);
            }
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
    pub fn read_wire(&self, pin: Pin) -> Option<bool> {
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

#[derive(Clone, Copy, Debug)]
struct NetworkId(usize);

struct Network {
    pins: HashSet<PinId>,
    state: bool,
    id: NetworkId,
}

impl Network {
    pub fn update(&mut self, pins: &Pins) {
        self.state = self
            .pins
            .iter()
            .any(|p| pins.get_state(*p).unwrap_or(false));
    }
}

#[derive(Default)]
struct Networks {
    networks: StableVec<Network>,
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

    pub fn get_state(&self, network: NetworkId) -> Option<bool> {
        self.networks
            .buffer
            .get(network.0)?
            .as_ref()
            .map(|n| n.state)
    }

    pub fn get_network(&self, pin: PinId) -> Option<NetworkId> {
        for network in self.networks.iter() {
            if network.pins.contains(&pin) {
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
        slot.set(Network {
            pins: HashSet::default(),
            state: false,
            id,
        });

        id
    }

    fn merge(&mut self, network_a: NetworkId, network_b: NetworkId) {
        let move_into;
        let move_from;

        match network_a.0.cmp(&network_b.0) {
            Ordering::Equal => panic!(
                "Cannot merge 2 identical networks. Attempted to merge {network_a:?} with itself."
            ),
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

        for pin in move_from.pins {
            move_into.pins.insert(pin);
        }
    }

    pub fn connect(&mut self, pin_a: PinId, pin_b: PinId) {
        let network_a = self.get_network(pin_a);
        let network_b = self.get_network(pin_b);

        match (network_a, network_b) {
            (None, None) => {
                let mut slot = self.networks.reserve();
                let id = NetworkId(slot.index);
                slot.set(Network {
                    pins: HashSet::from([pin_a, pin_b]),
                    state: false,
                    id,
                });
            }
            (Some(a), Some(b)) => self.merge(a, b),
            (Some(a), None) => self.add_pin(a, pin_b),
            (None, Some(b)) => self.add_pin(b, pin_a),
        }
    }

    fn add_pin(&mut self, network: NetworkId, pin: PinId) {
        self.networks
            .get_mut(network.0)
            .as_mut()
            .unwrap()
            .pins
            .insert(pin);
    }

    pub fn remove_pin(&mut self, pin: PinId) {
        let Some(network_id) = self.get_network(pin) else {
            return;
        };

        let network = self.networks.get_mut(network_id.0).unwrap();
        network.pins.remove(&pin);

        // if there is only one pin in the network or the network is empty, remove it to free up some processing time.
        if network.pins.len() <= 1 {
            self.networks.remove(network_id.0);
        }
    }
}
