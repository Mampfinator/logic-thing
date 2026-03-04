use core::f32;
use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
};

use macroquad::{input, prelude::*};
use petgraph::{
    Direction::{Incoming, Outgoing},
    prelude::{NodeIndex, StableDiGraph},
    visit::EdgeRef,
};

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
pub struct Resources {
    resources: HashMap<TypeId, Box<dyn Any>>,
}

impl Resources {
    pub fn insert_default<T: Default + 'static>(&mut self) -> Option<T> {
        self.insert(T::default())
    }

    pub fn insert<T: 'static>(&mut self, resource: T) -> Option<T> {
        self.resources
            .insert(TypeId::of::<T>(), Box::new(resource))
            .and_then(|v| v.downcast::<T>().ok())
            .map(|boxed| *boxed)
    }

    pub fn delete<T: 'static>(&mut self) -> Option<T> {
        self.resources
            .remove(&TypeId::of::<T>())
            .and_then(|v| v.downcast::<T>().ok())
            .map(|boxed| *boxed)
    }

    pub fn get<T: 'static>(&self) -> Option<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            .and_then(|any| any.downcast_ref())
    }

    pub fn get_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.resources
            .get_mut(&TypeId::of::<T>())
            .and_then(|any| any.downcast_mut())
    }

    pub fn get_mut_or_insert_default<T: Default + 'static>(&mut self) -> &mut T {
        if !self.resources.contains_key(&TypeId::of::<T>()) {
            self.insert_default::<T>();
        }
        self.get_mut().unwrap()
    }
}

#[derive(Default)]
pub struct GameObjects {
    objects: StableVec<GameObjectData>,
    state: StableVec<GameObjectState>,
    hierarchy: Hierarchy,
}

#[derive(Default)]
struct Hierarchy {
    indices: HashMap<ObjectId, NodeIndex>,
    roots: HashSet<NodeIndex>,
    graph: StableDiGraph<ObjectId, ()>,
}

#[derive(Debug, Clone, Copy)]
enum ParentError {
    AlreadyParented,
    ChildToParent,
}

#[derive(Debug, Clone, Copy)]
enum DeparentError {
    NoSuchNode,
    NoSuchRelationship,
}

impl Hierarchy {
    fn insert_root(&mut self, object: ObjectId) -> Option<NodeIndex> {
        if self.indices.contains_key(&object) {
            return None;
        }

        let index = self.graph.add_node(object);
        self.indices.insert(object, index);
        self.roots.insert(index);

        Some(index)
    }

    fn get_or_insert_node(&mut self, node: ObjectId) -> NodeIndex {
        if let Some(index) = self.indices.get(&node) {
            *index
        } else {
            self.insert_root(node).unwrap()
        }
    }

    pub fn set_parent(&mut self, child: ObjectId, parent: ObjectId) -> Result<(), ParentError> {
        let child = self.get_or_insert_node(child);
        let parent = self.get_or_insert_node(parent);

        if self.graph.edges_connecting(child, parent).count() > 0 {
            return Err(ParentError::AlreadyParented);
        } else if self.graph.edges_connecting(parent, child).count() > 0 {
            return Err(ParentError::ChildToParent);
        }

        self.graph.add_edge(child, parent, ());
        Ok(())
    }

    pub fn deparent(&mut self, node: ObjectId) -> Result<(), DeparentError> {
        let child = self.indices.get(&node).ok_or(DeparentError::NoSuchNode)?;
        let edge = self
            .graph
            .edges_directed(*child, Outgoing)
            .next()
            .ok_or(DeparentError::NoSuchRelationship)?
            .id();

        self.graph.remove_edge(edge);

        self.roots.insert(*child);

        Ok(())
    }

    fn get_children_indices(&self, parent: ObjectId) -> Option<Vec<NodeIndex>> {
        let index = *self.indices.get(&parent)?;
        fn visit(node: NodeIndex, vec: &mut Vec<NodeIndex>, graph: &StableDiGraph<ObjectId, ()>) {
            let children = graph.neighbors_directed(node, Incoming).collect::<Vec<_>>();
            vec.extend(children.iter().copied());

            for child in children.into_iter() {
                visit(child, vec, graph);
            }
        }

        let mut out = Vec::new();

        visit(index, &mut out, &self.graph);

        Some(out)
    }

    pub fn get_children(&self, parent: ObjectId) -> Option<Vec<ObjectId>> {
        self.get_children_indices(parent)?
            .into_iter()
            .map(|index| self.graph.node_weight(index).copied().unwrap())
            .collect::<Vec<_>>()
            .into()
    }

    pub fn remove_recursively(&mut self, parent: ObjectId) -> Option<Vec<ObjectId>> {
        self.get_children_indices(parent)?
            .into_iter()
            .map(|index| self.graph.remove_node(index).unwrap())
            .collect::<Vec<_>>()
            .into()
    }
}

#[derive(Default)]
struct Game {
    pub simulation: Simulation,
    pub game_objects: GameObjects,
    pub resources: Resources,
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

            if is_inside && !ctx.state.hovered {
                ctx.state.hovered = true;
                object.on_mouse_enter(&mut ctx, &mut self.simulation);
            }

            if !is_inside && ctx.state.hovered {
                ctx.state.hovered = false;
                object.on_mouse_exit(&mut ctx, &mut self.simulation);
            }

            if is_inside && clicked {
                object.on_click(&mut ctx, &mut self.simulation);
            }

            if is_inside && released {
                object.on_mouse_enter(&mut ctx, &mut self.simulation);
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
        for (id, object, state) in self.game_objects.iter() {
            object.render(
                &ObjectContext::new(state, id),
                &self.simulation,
                &self.game_objects,
            );
        }
    }
}

trait MakeGameObject: Chip {
    type Args;
    type Obj: GameObject + Hash;
    fn make_game_object(id: ChipId, args: Self::Args) -> Self::Obj;
}

// TODO: make derive macro instead
// eventually.
/// Implement [`MakeGameObject`] for a series of types.
/// Additional options per type include `Type as Other` where `Other` implements `From<ChipId>`,
/// and `Type as Other where Args = (...Args)` where `Other` implements a method `new(ChipId, Args)`.
#[macro_export]
macro_rules! impl_mgo {
    ($type:ty) => {
        impl $crate::MakeGameObject for $type {
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

    ($type:ty as $obj:ty where Args = $($args:ty),*) => {
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
            $type:ty $(as $obj:ty $(where Args = ($($args:ty),*))?)?
        ),+ $(,)?
    ) => {
        $(
            impl_mgo!(
                $type $(as $obj $(where Args = $($args),*)?)?
            );
        )+
    };
}

impl_mgo!(
    Clock,
    Counter8b,
    TieHigh,
    Led as LedObj where Args = (Color),
    NumericDisplay as NumericDisplayObj,
    CPU,
);

// TODO: custom metadata on GameObjectState?
/// (barely) copy type storing object metadata.
/// The first 64 bits are the index of the object in the internal vector, the next 64 can be arbitrary metadata,
/// and the last 8 are a type identifier.
/// This metadata is mainly used to find position data about other simulation objects during rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectId(usize);

struct GameObjectData {
    object: Box<dyn GameObject>,
    id: ObjectId,
    identifier: (TypeId, u64),
}

#[derive(Default)]
pub struct CommandBuffer {
    commands: Vec<Box<dyn ObjectCommand>>,
}

impl CommandBuffer {
    pub fn push<C: ObjectCommand>(&mut self, command: C) {
        self.commands.push(Box::new(command))
    }

    pub fn apply(
        &mut self,
        game_objects: &mut GameObjects,
        simulation: &mut Simulation,
        resources: &mut Resources,
    ) {
        for mut command in self.commands.drain(0..) {
            command.apply(game_objects, simulation, resources)
        }
    }
}

pub struct ObjectContextMut<'a, 'b> {
    state: &'a mut GameObjectState,
    id: ObjectId,
    commands: &'b mut CommandBuffer,
    resources: &'a mut Resources,
}

impl<'a, 'b> ObjectContextMut<'a, 'b> {
    pub fn new(
        state: &'a mut GameObjectState,
        id: ObjectId,
        commands: &'b mut CommandBuffer,
        resources: &'a mut Resources,
    ) -> Self {
        Self {
            state,
            id,
            commands,
            resources,
        }
    }

    pub fn mouse_world_pos(&self) -> Vec2 {
        self.resources
            .get::<Camera>()
            .unwrap()
            .get_mouse_world_pos()
    }

    pub fn resource_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.resources.get_mut::<T>()
    }

    pub fn resource<T: 'static>(&self) -> Option<&T> {
        self.resources.get()
    }

    pub fn push<C: ObjectCommand>(&mut self, command: C) -> &mut Self {
        self.commands.push(command);
        self
    }

    pub fn move_by(&mut self, offset: Vec2) -> &mut Self {
        self.push(MoveBy(self.id, offset))
    }

    pub fn set_shape(&mut self, shape: Shape) -> &mut Self {
        self.state.shape = Some(shape);
        self
    }

    pub fn set_draw_priority(&mut self, priority: usize) -> &mut Self {
        self.state.draw_priority = priority;
        self
    }

    pub fn despawn(&mut self) -> &mut Self {
        self.push(Despawn(self.id))
    }

    pub fn spawn_child<T: GameObject + Hash>(&mut self, child: T, position: Vec2) -> &mut Self {
        self.push(SpawnRelated::new(self.id, child, position))
    }
}

trait GetState {
    fn get_state(&self) -> &GameObjectState;

    fn hovered(&self) -> bool {
        self.get_state().hovered
    }

    fn position(&self) -> Vec2 {
        self.get_state().position
    }
}

impl GetState for ObjectContextMut<'_, '_> {
    #[inline(always)]
    fn get_state(&self) -> &GameObjectState {
        self.state
    }
}

impl GetState for ObjectContext<'_> {
    #[inline(always)]
    fn get_state(&self) -> &GameObjectState {
        self.state
    }
}

pub struct ObjectContext<'a> {
    state: &'a GameObjectState,
    id: ObjectId,
}

impl<'a> ObjectContext<'a> {
    pub fn new(state: &'a GameObjectState, id: ObjectId) -> Self {
        Self { state, id }
    }
}

pub trait ObjectCommand: 'static {
    fn apply(
        &mut self,
        objects: &mut GameObjects,
        simulation: &mut Simulation,
        resources: &mut Resources,
    );
}

struct MoveBy(ObjectId, Vec2);

impl ObjectCommand for MoveBy {
    fn apply(&mut self, objects: &mut GameObjects, _: &mut Simulation, _: &mut Resources) {
        objects.move_by(self.0, self.1);
    }
}

struct Despawn(ObjectId);

impl ObjectCommand for Despawn {
    fn apply(&mut self, objects: &mut GameObjects, _: &mut Simulation, _: &mut Resources) {
        objects.despawn(self.0);
    }
}

struct SpawnRelated<C: GameObject>(ObjectId, Option<C>, Vec2);

impl<C: GameObject> SpawnRelated<C> {
    pub fn new(id: ObjectId, object: C, position: Vec2) -> Self {
        Self(id, Some(object), position)
    }
}

impl<C: GameObject + Hash> ObjectCommand for SpawnRelated<C> {
    fn apply(
        &mut self,
        objects: &mut GameObjects,
        simulation: &mut Simulation,
        resources: &mut Resources,
    ) {
        objects.insert_child(
            self.0,
            self.1.take().unwrap(),
            self.2,
            simulation,
            resources,
        );
    }
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

    pub fn insert_child<O: GameObject + Hash>(
        &mut self,
        parent: ObjectId,
        child: O,
        position: Vec2,
        simulation: &mut Simulation,
        resources: &mut Resources,
    ) -> ObjectId {
        let child = self.insert(child, position, simulation, resources);
        self.hierarchy.set_parent(child, parent).unwrap();
        child
    }

    pub fn insert<O: GameObject + Hash>(
        &mut self,
        object: O,
        position: Vec2,
        simulation: &mut Simulation,
        resources: &mut Resources,
    ) -> ObjectId {
        let mut slot = self.objects.reserve();

        let id = ObjectId(slot.index);

        let mut hasher = DefaultHasher::default();
        object.hash(&mut hasher);

        let hash = hasher.finish();

        slot.set(GameObjectData {
            object: Box::new(object),
            id,
            identifier: (TypeId::of::<O>(), hash),
        });

        let mut state = GameObjectState {
            position,
            draw_priority: 0,
            shape: None,
            hovered: false,
        };

        let mut buffer = CommandBuffer::default();

        let mut ctx: ObjectContextMut =
            ObjectContextMut::new(&mut state, id, &mut buffer, resources);

        slot.slot
            .as_mut()
            .unwrap()
            .object
            .start(&mut ctx, simulation);

        let state_id = self.state.push(state);

        debug_assert_eq!(id.0, state_id);

        buffer.apply(self, simulation, resources);

        id
    }

    /// Use the `GameObject` type's TypeId and Hash to find its corresponding metadata.
    pub fn find_state<O: GameObject + Hash>(&self, object: &O) -> Option<&GameObjectState> {
        let type_id = TypeId::of::<O>();

        let mut hasher = DefaultHasher::new();
        object.hash(&mut hasher);
        let hash = hasher.finish();

        let index = self
            .objects
            .iter()
            .find(|o| o.identifier.0 == type_id && o.identifier.1 == hash)?
            .id
            .0;

        self.state.get(index)
    }

    pub fn update(
        &mut self,
        simulation: &mut Simulation,
        _camera: &mut Camera,
        resources: &mut Resources,
    ) {
        let mut buffer = CommandBuffer::default();

        for (id, object, state) in self.iter_mut() {
            let mut ctx = ObjectContextMut::new(state, id, &mut buffer, resources);
            object.update(&mut ctx, simulation);
        }

        buffer.apply(self, simulation, resources);
    }

    pub fn despawn(&mut self, id: ObjectId) {
        self.objects.remove(id.0);
        self.state.remove(id.0);
        self.hierarchy.remove_recursively(id);
    }

    pub fn render(&self, simulation: &Simulation) {
        let mut objects = self.iter().collect::<Vec<_>>();

        objects.sort_by(|(_, _, a), (_, _, b)| a.draw_priority.cmp(&b.draw_priority));

        for (id, object, state) in objects.into_iter() {
            object.render(&ObjectContext::new(state, id), simulation, self)
        }
    }

    pub fn iter_mut(
        &mut self,
    ) -> impl Iterator<Item = (ObjectId, &mut dyn GameObject, &mut GameObjectState)> {
        self.objects
            .iter_mut()
            .zip(self.state.iter_mut())
            .map(|(object, state)| (object.id, &mut *object.object, state))
    }

    pub fn iter(&self) -> impl Iterator<Item = (ObjectId, &dyn GameObject, &GameObjectState)> {
        self.objects
            .iter()
            .zip(self.state.iter())
            .map(|(object, state)| (object.id, &*object.object, state))
    }

    /// Moves an object by `offset`. Also propagates the movement to all its children.
    fn move_by(&mut self, object: ObjectId, offset: Vec2) {
        self.state.get_mut(object.0).unwrap().position += offset;
        if let Some(shape) = self.state.get_mut(object.0).unwrap().shape.as_mut() {
            shape.move_by(offset);
        }

        let Some(children) = self.hierarchy.get_children(object) else {
            return;
        };

        for child in children {
            self.state.get_mut(child.0).unwrap().position += offset;
            if let Some(shape) = self.state.get_mut(child.0).unwrap().shape.as_mut() {
                shape.move_by(offset);
            }
        }
    }
}

pub enum Shape {
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

    pub fn move_by(&mut self, offset: Vec2) {
        match self {
            Self::Rectangle(rect) => {
                *rect = rect.offset(offset);
            }
            Self::Circle(circle) => {
                *circle = circle.offset(offset);
            }
        }
    }
}

pub struct GameObjectState {
    position: Vec2,
    shape: Option<Shape>,
    /// Objects with higher draw priority are drawn ***later***, thus above objects with a lower priority.
    /// Mind that, within a priority group, there is no guarantee about draw ordering.
    draw_priority: usize,
    hovered: bool,
}

pub trait GameObject: 'static {
    #[allow(unused)]
    fn start(&mut self, state: &mut ObjectContextMut, simulation: &Simulation) {}
    fn render(&self, context: &ObjectContext, simulation: &Simulation, objects: &GameObjects);
    #[allow(unused)]
    fn update(&mut self, state: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_mouse_enter(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_click_released(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}
}

impl GameObject for ChipId {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
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
        let mouse_pos = ctx.mouse_world_pos();
        ctx.resource_mut::<ChipClickOffset>().unwrap().0 = ctx.position() - mouse_pos;
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, _: &mut Simulation) {
        if ctx.hovered()
            && input::is_key_down(KeyCode::LeftAlt)
            && input::is_mouse_button_down(MouseButton::Left)
        {
            let mouse_pos = ctx.mouse_world_pos();
            let offset = ctx.resource::<ChipClickOffset>().unwrap();
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

impl GameObject for PinId {
    fn start(&mut self, state: &mut ObjectContextMut, _: &Simulation) {
        state.set_draw_priority(2);

        state.set_shape(Shape::Circle(Circle {
            x: state.position().x,
            y: state.position().y,
            r: TILE_SIZE / 4.,
        }));
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        let selection = ctx.resource_mut::<PinSelection>().unwrap();
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
}

#[derive(Clone, Copy, Debug)]
pub struct InvalidMarker;

impl GameObject for NetworkId {
    fn start(&mut self, state: &mut ObjectContextMut, _: &Simulation) {
        state.set_draw_priority(1);
    }

    fn update(&mut self, state: &mut ObjectContextMut, simulation: &mut Simulation) {
        if simulation.networks.get(*self).is_none() {
            state.despawn();
        }
    }

    fn render(&self, _: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        let network = simulation.networks.get(*self).unwrap();

        let color = if network.state { RED } else { GREEN };

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
    game.resources.insert_default::<ChipClickOffset>();

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
            uvec2(4, 8),
            (0..7).map(|idx| (Pin::Left(idx), PinDef::new(format!("C{idx}")))),
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
        let instance = simulation.chips.get(self.0).unwrap();

        for (pos, pin) in instance.pins_as_positions() {
            let offset = pos.get_pin_tile_offset(instance.size);

            ctx.spawn_child(pin, ctx.position() + offset);
        }
    }

    fn render(&self, state: &ObjectContext, simulation: &Simulation, _: &GameObjects) {
        let instance = simulation.chips.get(self.0).unwrap();
        let size = instance.size.as_vec2() * TILE_SIZE;
        let position = state.position();

        draw_rectangle(position.x, position.y, size.x, size.y, DARKGRAY);
        draw_rectangle_lines(position.x, position.y, size.x, size.y, 1., BLACK);

        let number = instance
            .pins
            .iter()
            .enumerate()
            .filter_map(|(index, &id)| id.map(|id| (index, id)))
            .filter_map(|(index, id)| {
                simulation
                    .networks
                    .get_network(id)
                    .and_then(|network| simulation.networks.get_state(network).map(|n| (index, n)))
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
