use core::f32;

use macroquad::{input, prelude::*};

use crate::{
    chips::{button, rom, switch},
    simulation::{
        Chip, ChipId, NetworkId, Pin, PinDef, PinId, PinLayout, PinsState, Simulation, StableVec,
    },
};

pub const TILE_SIZE: f32 = 16.0;

pub mod chips;
pub mod simulation;

use chips::cpu::{CPU, DATA_PINS};

#[derive(Default)]
struct GameObjects {
    objects: StableVec<GameObjectData>,
    state: StableVec<GameObjectState>,
}

#[derive(Default)]
struct Game {
    pub simulation: Simulation,
    pub game_objects: GameObjects,
}

impl Game {
    /// ## Example
    /// ```
    /// let mut game = Game::default()
    ///
    /// let [clock, counter, led] = game.place_chips((
    ///   (Clock::new(100), vec2(100., 100.)),
    ///   (Counter8b, vec2(200., 100.)),
    ///   // this "RED" here is a rendering option. But it can figure it out. :)
    ///   (Led, vec2(300., 100.), RED),
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
    ) -> (ChipId, ObjectId) {
        let id = self.simulation.place_chip(chip);

        let object = C::make_game_object(id, args);
        let oid = self.game_objects.insert(object, position, &self.simulation);

        (id, oid)
    }
}

trait MakeGameObject: Chip {
    type Args;
    type Obj: GameObject;
    fn make_game_object(id: ChipId, args: Self::Args) -> Self::Obj;
}

/// Implement [`MakeGameObject`] for a series of types.
/// Additional options per type include `Type as Other` where `Other` implements `From<ChipId>`,
/// and `Type as Other where Args: (...Args)` where `Other` implements a method `new(ChipId, Args)`.
#[macro_export]
macro_rules! impl_mgo {
    ($type:ty) => {
        impl crate::MakeGameObject for $type {
            type Args = ();
            type Obj = crate::ChipId;
            fn make_game_object(id: crate::ChipId, _args: ()) -> crate::ChipId {
                id
            }
        }
    };

    ($type:ty as $obj:ty) => {
        impl crate::MakeGameObject for $type {
            type Args = ();
            type Obj = $obj;
            fn make_game_object(id: ChipId, _args: ()) -> Self::Obj {
                <$obj as From::<_>>::from(id)
            }

        }
    };

    ($type:ty as $obj:ty where Args: $($args:ty),*) => {
        impl crate::MakeGameObject for $type {
            #[allow(unused_parens)]
            type Args = ($($args),*);
            type Obj = $obj;
            #[allow(unused_parens)]
            fn make_game_object(id: ChipId, args: ($($args),*)) -> Self::Obj {
                <$obj>::new(id, args)
            }
        }
    };

    (
        $(
            $type:ty $(as $obj:ty $(where Args: ($($args:ty),*))?)?
        ),+ $(,)?
    ) => {
        $(
            impl_mgo!(
                $type $(as $obj $(where Args: $($args),*)?)?
            );
        )+
    };
}

impl_mgo!(
    Clock,
    Counter8b,
    TieHigh,
    Led as LedRenderer where Args: (Color),
    NumericDisplay as NumericDisplayRenderer,
    CPU,
);

// TODO: custom metadata on GameObjectState?

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
                return Some(self.objects.get(index).unwrap().id);
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

    pub fn update(&mut self, simulation: &mut Simulation, camera: &mut Camera) {
        let mut despawned = Vec::new();

        for (object, state) in self.objects.iter_mut().zip(self.state.iter_mut()) {
            object.object.update(state, simulation, camera);
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
    fn update(
        &mut self,
        state: &mut GameObjectState,
        simulation: &mut Simulation,
        camera: &mut Camera,
    ) {
    }
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

    fn update(&mut self, state: &mut GameObjectState, simulation: &mut Simulation, _: &mut Camera) {
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

struct LedRenderer(pub ChipId, pub Color);

impl LedRenderer {
    pub fn new(chip: ChipId, color: Color) -> Self {
        Self(chip, color)
    }
}

impl GameObject for LedRenderer {
    fn start(
        &mut self,
        state: &mut GameObjectState,
        simulation: &Simulation,
        objects: &mut GameObjects,
    ) {
        let instance = simulation.chips.get(self.0).unwrap();

        let (pos, pin) = instance.pins_as_positions().next().unwrap();

        objects.insert(pin, state.position - vec2(TILE_SIZE, 0.), simulation);
    }

    fn make_oid_meta(&self) -> (usize, u8) {
        (self.0.0, 4)
    }

    fn render(&self, state: &GameObjectState, simulation: &Simulation, objects: &GameObjects) {
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

        let pos = state.position;

        draw_circle(pos.x, pos.y, TILE_SIZE, color);
        draw_circle_lines(pos.x, pos.y, TILE_SIZE, 1., BLACK);
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

    let mut game = Game::default();

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
    // let (clock, _) = game.place_chip(Clock::new(5), vec2(100., 100.), ());

    // let (counter, _) = game.place_chip(Counter8b::default(), vec2(300., 100.), ());

    // game.simulation
    //     .connect(clock, Pin::Right(1), counter, Pin::Left(4));

    // let (counter2, _) = game.place_chip(Counter8b::default(), vec2(300., 250.), ());

    // game.simulation
    //     .connect(counter, Pin::Right(7), counter2, Pin::Left(4));

    // let (led, _) = game.place_chip(Led, vec2(500., 100.), RED);

    // game.simulation
    //     .connect(counter, Pin::Right(5), led, Pin::Left(0));

    let [cpu, high, clock] = game.place_chips((
        (CPU::default(), vec2(100., 100.)),
        (TieHigh, vec2(-25., 100.)),
        (Clock::new(1), vec2(-25., 116.)),
    ));

    game.simulation
        .connect(cpu.0, Pin::Left(0), high.0, Pin::Right(0));
    game.simulation
        .connect(cpu.0, Pin::Left(2), clock.0, Pin::Right(1));

    game.simulation
        .connect(cpu.0, DATA_PINS[0], high.0, Pin::Right(0));

    let mut rom = [0; 256];
    for i in 0..u8::MAX {
        rom[i as usize] = i;
    }

    let [rom, switches, high_2, button] = game.place_chips((
        (rom::ROM::from(rom), vec2(500., 100.)),
        (switch::Switch::new(8), vec2(300., 116.), 8),
        (TieHigh, vec2(348., 100. - TILE_SIZE)),
        (button::Button, vec2(348., 100.)),
    ));

    for i in 0..8 {
        game.simulation
            .connect(rom.0, Pin::Left(i + 1), switches.0, Pin::Right(i));
    }

    game.simulation
        .connect(high_2.0, Pin::Right(0), rom.0, Pin::Left(0));

    game.simulation
        .connect(button.0, Pin::Right(0), rom.0, Pin::Right(0));

    let mut selected_pin: Option<PinId> = None;

    for _ in 0.. {
        clear_background(SKYBLUE);

        game.simulation.tick();

        if input::is_mouse_button_pressed(MouseButton::Left) {
            let world_pos = camera
                .camera
                .screen_to_world(input::mouse_position().into());

            if let Some(object) = game.game_objects.find_by_position(world_pos)
                && let Ok(pin) = PinId::try_from(object)
            {
                if let Some(first_pin) = selected_pin {
                    game.simulation.networks.toggle_connect(first_pin, pin);
                    selected_pin = None;
                } else {
                    selected_pin = Some(pin);
                }
            }
        } else if input::is_mouse_button_pressed(MouseButton::Right) {
            selected_pin = None;
        }

        if let Some(pin) = selected_pin {
            let pin = game.simulation.pins.get(pin).unwrap();
            let label = pin
                .label
                .as_ref()
                .cloned()
                .unwrap_or(format!("Pin {}", pin.id.0));

            draw_text(&format!("Pin selected: {}", label), 0., 100., 32., WHITE);
        }

        // sync newly created networks, if any.
        for network in game.simulation.networks.ids() {
            if game.game_objects.find_by_meta(&network).is_none() {
                game.game_objects
                    .insert(network, vec2(0., 0.), &game.simulation);
            }
        }

        camera.update();

        game.game_objects.update(&mut game.simulation, &mut camera);
        game.game_objects.render(&game.simulation);

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
            uvec2(4, 8),
            (0..7).map(|idx| (Pin::Left(idx), PinDef::new(format!("C{idx}")))),
        )
    }

    fn update(&mut self, _: &mut PinsState) {}
}

struct NumericDisplayRenderer(ChipId);

impl From<ChipId> for NumericDisplayRenderer {
    fn from(value: ChipId) -> Self {
        Self(value)
    }
}

impl GameObject for NumericDisplayRenderer {
    fn start(
        &mut self,
        state: &mut GameObjectState,
        simulation: &Simulation,
        objects: &mut GameObjects,
    ) {
        let instance = simulation.chips.get(self.0).unwrap();

        for (pos, pin) in instance.pins_as_positions() {
            let offset = pos.get_pin_tile_offset(instance.size);

            objects.insert(pin, state.position + offset, simulation);
        }
    }

    fn render(&self, state: &GameObjectState, simulation: &Simulation, objects: &GameObjects) {
        let instance = simulation.chips.get(self.0).unwrap();
        let size = instance.size.as_vec2() * TILE_SIZE;
        let position = state.position;

        draw_rectangle(position.x, position.y, size.x, size.y, DARKGRAY);
        draw_rectangle_lines(position.x, position.y, size.x, size.y, 1., BLACK);

        let number = instance
            .pins
            .iter()
            .enumerate()
            .filter_map(|(index, &id)| id.map(|id| (index, id)))
            .filter_map(|(index, id)| {
                simulation.networks.get_network(id).and_then(|network| {
                    (simulation.networks.get_state(network).map(|n| (index, n)))
                })
            })
            .map(|(index, state)| (state.is_high() as u8) << index)
            .fold(0_u8, |acc, item| acc & item);

        println!("{number}");
    }
}

trait SplitForMgo<C: MakeGameObject> {
    fn split_for_mgo(self) -> (C, Vec2, <C as MakeGameObject>::Args);
}

macro_rules! impl_split_for_mgo {
    ($($name:ident),*) => {
        #[allow(unused_parens)]
        impl<C, $($name),*> SplitForMgo<C> for (C, Vec2, $ ( $name ),*)
        where
            C: MakeGameObject<Args = ($( $name ),*)>, {
            fn split_for_mgo(self) -> (C, Vec2, C::Args) {
                #[allow(non_snake_case)]
                let (c, pos, $($name),*) = self;
                (c, pos, ($ ($name),*))
            }
        }
    }
}

impl_split_for_mgo!();
impl_split_for_mgo!(A0);
impl_split_for_mgo!(A0, A1);
impl_split_for_mgo!(A0, A1, A2);
impl_split_for_mgo!(A0, A1, A2, A3);
impl_split_for_mgo!(A0, A1, A2, A3, A4);
impl_split_for_mgo!(A0, A1, A2, A3, A4, A5);
impl_split_for_mgo!(A0, A1, A2, A3, A4, A5, A6);
impl_split_for_mgo!(A0, A1, A2, A3, A4, A5, A6, A7);

// TODO: this really needs to be a macro. But macros are hard.
trait PlaceMgos<T, const N: usize> {
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)>;
}

impl<C: MakeGameObject + 'static, MGO: SplitForMgo<C>> PlaceMgos<C, 1> for MGO {
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let (c, pos, args) = self.split_for_mgo();
        vec![game.place_chip(c, pos, args)]
    }
}

impl<
    C0: MakeGameObject + 'static,
    MGO0: SplitForMgo<C0>,
    C1: MakeGameObject + 'static,
    MGO1: SplitForMgo<C1>,
> PlaceMgos<(C0, C1), 2> for (MGO0, MGO1)
{
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let mut out = Vec::new();
        let (c, pos, args) = self.0.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        out
    }
}

impl<
    C0: MakeGameObject + 'static,
    MGO0: SplitForMgo<C0>,
    C1: MakeGameObject + 'static,
    MGO1: SplitForMgo<C1>,
    C2: MakeGameObject + 'static,
    MGO2: SplitForMgo<C2>,
> PlaceMgos<(C0, C1, C2), 3> for (MGO0, MGO1, MGO2)
{
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let mut out = Vec::new();
        let (c, pos, args) = self.0.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        out
    }
}

impl<
    C0: MakeGameObject + 'static,
    MGO0: SplitForMgo<C0>,
    C1: MakeGameObject + 'static,
    MGO1: SplitForMgo<C1>,
    C2: MakeGameObject + 'static,
    MGO2: SplitForMgo<C2>,
    C3: MakeGameObject + 'static,
    MGO3: SplitForMgo<C3>,
> PlaceMgos<(C0, C1, C2, C3), 4> for (MGO0, MGO1, MGO2, MGO3)
{
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let mut out = Vec::new();
        let (c, pos, args) = self.0.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        out
    }
}

impl<
    C0: MakeGameObject + 'static,
    MGO0: SplitForMgo<C0>,
    C1: MakeGameObject + 'static,
    MGO1: SplitForMgo<C1>,
    C2: MakeGameObject + 'static,
    MGO2: SplitForMgo<C2>,
    C3: MakeGameObject + 'static,
    MGO3: SplitForMgo<C3>,
    C4: MakeGameObject + 'static,
    MGO4: SplitForMgo<C4>,
> PlaceMgos<(C0, C1, C2, C3, C4), 5> for (MGO0, MGO1, MGO2, MGO3, MGO4)
{
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let mut out = Vec::new();
        let (c, pos, args) = self.0.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        out
    }
}

impl<
    C0: MakeGameObject + 'static,
    MGO0: SplitForMgo<C0>,
    C1: MakeGameObject + 'static,
    MGO1: SplitForMgo<C1>,
    C2: MakeGameObject + 'static,
    MGO2: SplitForMgo<C2>,
    C3: MakeGameObject + 'static,
    MGO3: SplitForMgo<C3>,
    C4: MakeGameObject + 'static,
    MGO4: SplitForMgo<C5>,
    C5: MakeGameObject + 'static,
    MGO5: SplitForMgo<C4>,
> PlaceMgos<(C0, C1, C2, C3, C4, C5), 6> for (MGO0, MGO1, MGO2, MGO3, MGO4, MGO5)
{
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let mut out = Vec::new();
        let (c, pos, args) = self.0.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.5.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        out
    }
}
impl<
    C0: MakeGameObject + 'static,
    MGO0: SplitForMgo<C0>,
    C1: MakeGameObject + 'static,
    MGO1: SplitForMgo<C1>,
    C2: MakeGameObject + 'static,
    MGO2: SplitForMgo<C2>,
    C3: MakeGameObject + 'static,
    MGO3: SplitForMgo<C3>,
    C4: MakeGameObject + 'static,
    MGO4: SplitForMgo<C4>,
    C5: MakeGameObject + 'static,
    MGO5: SplitForMgo<C5>,
    C6: MakeGameObject + 'static,
    MGO6: SplitForMgo<C6>,
> PlaceMgos<(C0, C1, C2, C3, C4, C5, C6), 7> for (MGO0, MGO1, MGO2, MGO3, MGO4, MGO5, MGO6)
{
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let mut out = Vec::new();
        let (c, pos, args) = self.0.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.5.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.6.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        out
    }
}

impl<
    C0: MakeGameObject + 'static,
    MGO0: SplitForMgo<C0>,
    C1: MakeGameObject + 'static,
    MGO1: SplitForMgo<C1>,
    C2: MakeGameObject + 'static,
    MGO2: SplitForMgo<C2>,
    C3: MakeGameObject + 'static,
    MGO3: SplitForMgo<C3>,
    C4: MakeGameObject + 'static,
    MGO4: SplitForMgo<C4>,
    C5: MakeGameObject + 'static,
    MGO5: SplitForMgo<C5>,
    C6: MakeGameObject + 'static,
    MGO6: SplitForMgo<C6>,
    C7: MakeGameObject + 'static,
    MGO7: SplitForMgo<C7>,
> PlaceMgos<(C0, C1, C2, C3, C4, C5, C6, C7), 8>
    for (MGO0, MGO1, MGO2, MGO3, MGO4, MGO5, MGO6, MGO7)
{
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let mut out = Vec::new();
        let (c, pos, args) = self.0.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.5.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.6.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        let (c, pos, args) = self.7.split_for_mgo();
        out.push(game.place_chip(c, pos, args));
        out
    }
}
