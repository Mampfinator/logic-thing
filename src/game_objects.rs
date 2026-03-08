use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
};

use macroquad::math::{Circle, IVec2, Rect, Vec2};
use macroquad::prelude::{Color, draw_line};
use petgraph::{
    Direction::{Incoming, Outgoing},
    graph::NodeIndex,
    prelude::StableDiGraph,
    visit::EdgeRef,
};

use crate::{
    Camera, Game, TILE_SIZE,
    simulation::{Chip, ChipId, Simulation, StableVec},
};

#[derive(Default)]
pub struct TypeMap {
    resources: HashMap<TypeId, Box<dyn Any>>,
}

impl TypeMap {
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
struct DrawLayers {
    layers: HashMap<usize, HashSet<ObjectId>>,
}

impl DrawLayers {
    pub fn iter_ordered(&self) -> impl Iterator<Item = ObjectId> {
        // is there any way to skip this allocation?
        let mut keys_sorted = self.layers.keys().copied().collect::<Vec<_>>();
        keys_sorted.sort_by(|a, b| a.cmp(b));
        keys_sorted
            .into_iter()
            .flat_map(|key| self.layers.get(&key).unwrap().iter().copied())
    }

    pub fn set_layer(&mut self, object: ObjectId, layer: usize) {
        for (&index, layer_set) in self.layers.iter_mut() {
            if layer_set.contains(&object) {
                if index == layer {
                    return;
                }

                layer_set.remove(&object);
                break;
            }
        }

        if let Some(layer) = self.layers.get_mut(&layer) {
            layer.insert(object);
        } else {
            let set = HashSet::from([object]);
            self.layers.insert(layer, set);
        }
    }
}

#[derive(Default)]
pub struct GameObjects {
    objects: StableVec<GameObjectData>,
    state: StableVec<GameObjectState>,
    hierarchy: Hierarchy,
    draw_layers: DrawLayers,
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

pub trait MakeGameObject: Chip {
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
        impl $crate::game_objects::MakeGameObject for $type {
            type Args = ();
            type Obj = crate::ChipId;
            fn make_game_object(id: crate::ChipId, _args: ()) -> crate::ChipId {
                id
            }
        }
    };

    ($type:ty as $obj:ty) => {
        impl crate::game_objects::MakeGameObject for $type {
            type Args = ();
            type Obj = $obj;
            fn make_game_object(id: ChipId, _args: ()) -> Self::Obj {
                <$obj as From::<_>>::from(id)
            }

        }
    };

    ($type:ty as $obj:ty where Args = $($args:ty),*) => {
        impl crate::game_objects::MakeGameObject for $type {
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
        resources: &mut TypeMap,
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
    resources: &'a mut TypeMap,
}

impl<'a, 'b> ObjectContextMut<'a, 'b> {
    pub fn new(
        state: &'a mut GameObjectState,
        id: ObjectId,
        commands: &'b mut CommandBuffer,
        resources: &'a mut TypeMap,
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

    pub fn get_resource_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.resources.get_mut::<T>()
    }

    pub fn resource_mut<T: 'static>(&mut self) -> &mut T {
        self.get_resource_mut().unwrap()
    }

    pub fn get_resource<T: 'static>(&self) -> Option<&T> {
        self.resources.get()
    }

    pub fn resource<T: 'static>(&self) -> &T {
        self.get_resource().unwrap()
    }

    pub fn data_mut<T: 'static>(&mut self) -> &mut T {
        self.get_data_mut().unwrap()
    }

    pub fn get_data_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.state.custom_data.get_mut()
    }

    pub fn data_mut_or_default<T: Default + 'static>(&mut self) -> &mut T {
        self.state.custom_data.get_mut_or_insert_default()
    }

    pub fn delete_data<T: 'static>(&mut self) {
        self.state.custom_data.delete::<T>();
    }

    pub fn insert_data<T: 'static>(&mut self, data: T) {
        self.state.custom_data.insert(data);
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

    pub fn despawn(&mut self) -> &mut Self {
        self.push(Despawn(self.id))
    }

    pub fn spawn_child<T: GameObject + Hash>(&mut self, child: T, position: Vec2) -> &mut Self {
        self.push(SpawnRelated::new(self.id, child, position))
    }

    pub fn set_hovered(&mut self, hovered: bool) -> &mut Self {
        self.state.hovered = hovered;
        self
    }

    pub fn set_layer(&mut self, layer: usize) -> &mut Self {
        self.push(SetLayer(self.id, layer))
    }
}

pub trait GetState {
    fn get_state(&self) -> &GameObjectState;

    fn hovered(&self) -> bool {
        self.get_state().hovered
    }

    fn position(&self) -> Vec2 {
        self.get_state().position
    }

    fn data<T: 'static>(&self) -> &T {
        self.get_data().unwrap()
    }

    fn get_data<T: 'static>(&self) -> Option<&T> {
        self.get_state().custom_data.get()
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
        resources: &mut TypeMap,
    );
}

struct SetLayer(pub ObjectId, pub usize);

impl ObjectCommand for SetLayer {
    fn apply(&mut self, objects: &mut GameObjects, _: &mut Simulation, _: &mut TypeMap) {
        objects.draw_layers.set_layer(self.0, self.1);
    }
}

struct MoveBy(ObjectId, Vec2);

impl ObjectCommand for MoveBy {
    fn apply(&mut self, objects: &mut GameObjects, _: &mut Simulation, _: &mut TypeMap) {
        objects.move_by(self.0, self.1);
    }
}

struct Despawn(ObjectId);

impl ObjectCommand for Despawn {
    fn apply(&mut self, objects: &mut GameObjects, _: &mut Simulation, _: &mut TypeMap) {
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
        resources: &mut TypeMap,
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
        resources: &mut TypeMap,
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
        resources: &mut TypeMap,
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
            shape: None,
            hovered: false,
            custom_data: TypeMap::default(),
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
        resources: &mut TypeMap,
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
        for (id, object, state) in self.draw_layers.iter_ordered().map(|id| {
            (
                id,
                &*self.objects.get(id.0).unwrap().object,
                self.state.get(id.0).unwrap(),
            )
        }) {
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
    pub position: Vec2,
    pub shape: Option<Shape>,
    pub hovered: bool,
    pub custom_data: TypeMap,
}

pub trait GameObject: 'static {
    #[allow(unused)]
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {}
    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, objects: &GameObjects);
    #[allow(unused)]
    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_mouse_enter(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}

    #[allow(unused)]
    fn on_click_released(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub spacing: f32,
    pub half_extent: i32,
    pub color: Color,
    pub axis_color: Color,
    pub axis_width: f32,
    pub line_width: f32,
}

impl Grid {
    pub fn new(spacing: f32, half_extent: i32, color: Color) -> Self {
        Self {
            spacing,
            half_extent,
            color,
            axis_color: color,
            axis_width: 2.0,
            line_width: 1.0,
        }
    }
}

impl Hash for Grid {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.spacing.to_bits().hash(state);
        self.half_extent.hash(state);
        self.color.r.to_bits().hash(state);
        self.color.g.to_bits().hash(state);
        self.color.b.to_bits().hash(state);
        self.color.a.to_bits().hash(state);
        self.axis_color.r.to_bits().hash(state);
        self.axis_color.g.to_bits().hash(state);
        self.axis_color.b.to_bits().hash(state);
        self.axis_color.a.to_bits().hash(state);
        self.axis_width.to_bits().hash(state);
        self.line_width.to_bits().hash(state);
    }
}

impl GameObject for Grid {
    fn start(&mut self, state: &mut ObjectContextMut, _: &Simulation) {
        state.set_layer(0);
    }

    fn render(&self, _: &ObjectContext, _: &Simulation, _: &GameObjects) {
        let extent = self.half_extent as f32 * self.spacing;

        for i in -self.half_extent..=self.half_extent {
            let offset = i as f32 * self.spacing;
            let is_axis = i == 0;

            let color = if is_axis { self.axis_color } else { self.color };
            let width = if is_axis {
                self.axis_width
            } else {
                self.line_width
            };

            draw_line(-extent, offset, extent, offset, width, color);
            draw_line(offset, -extent, offset, extent, width, color);
        }
    }
}

trait SplitForMgo<C: MakeGameObject> {
    fn split_for_mgo(self) -> (C, IVec2, <C as MakeGameObject>::Args);
}

macro_rules! impl_split_for_mgo {
    ($($name:ident),*) => {
        #[allow(unused_parens)]
        impl<C, $($name),*> SplitForMgo<C> for (C, IVec2, $ ( $name ),*)
        where
            C: MakeGameObject<Args = ($( $name ),*)>, {
            fn split_for_mgo(self) -> (C, IVec2, C::Args) {
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
pub(super) trait PlaceMgos<T, const N: usize> {
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)>;
}

impl<C: MakeGameObject + 'static, MGO: SplitForMgo<C>> PlaceMgos<C, 1> for MGO {
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let (c, pos, args) = self.split_for_mgo();
        vec![game.place_chip(c, pos.as_vec2() * TILE_SIZE, args)]
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
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
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
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
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
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
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
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
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
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.5.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
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
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.5.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.6.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
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
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.1.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.2.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.3.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.4.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.5.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.6.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        let (c, pos, args) = self.7.split_for_mgo();
        out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
        out
    }
}
