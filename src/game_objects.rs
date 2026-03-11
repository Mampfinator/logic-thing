use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    hash::{DefaultHasher, Hash, Hasher},
};

use macroquad::{
    input::{self, KeyCode, MouseButton},
    math::{Circle, IVec2, Rect, Vec2, vec2},
    prelude::{
        BLACK, Color, DARKGRAY, GRAY, LIGHTGRAY, TextParams, WHITE, draw_circle, draw_circle_lines,
        draw_line, draw_rectangle, draw_rectangle_lines, draw_text, draw_text_ex, screen_height,
        screen_width, set_default_camera,
    },
};
use petgraph::{Direction::Incoming, graph::NodeIndex, prelude::StableDiGraph};

use crate::{
    Camera, Game, Resource, Resources, TILE_SIZE,
    simulation::{Chip, ChipId, Simulation, StableVec},
};

mod chip_catalog;
use chip_catalog::{
    CHIP_CATALOG, hit_test_menu_item, hotkey_to_catalog_index, menu_hotkey_label,
    placement_menu_layout, placement_origin_from_cursor,
};
pub use chip_catalog::{ChipTemplate, PlacementUiState, UiInputResult};

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

    pub fn scoped<T: 'static, F>(&mut self, f: F)
    where
        F: FnOnce(&mut T, &mut Self),
    {
        let Some(mut value) = self.delete::<T>() else {
            return;
        };

        f(&mut value, self);
        self.insert(value);
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
        keys_sorted.sort();
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

    pub fn remove(&mut self, object: ObjectId) {
        for layer in self.layers.values_mut() {
            if layer.remove(&object) {
                return;
            }
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
            .map(|index| {
                let object = self.graph.remove_node(index).unwrap();
                self.indices.remove(&object);
                object
            })
            .collect::<Vec<_>>()
            .into()
    }
}

pub trait MakeGameObject: Chip {
    type Args;
    type Obj: GameObject + Hash;
    fn make_game_object(id: ChipId, args: Self::Args) -> Self::Obj;
}

pub(crate) fn spawn_make_object<C: MakeGameObject + 'static>(
    simulation: &mut Simulation,
    game_objects: &mut GameObjects,
    resources: &mut Resources,
    chip: C,
    position: Vec2,
    args: <C as MakeGameObject>::Args,
) -> (ChipId, ObjectId)
where
    <C as MakeGameObject>::Obj: Hash,
{
    let id = simulation.place_chip(chip);

    let object = C::make_game_object(id, args);
    let object_id = game_objects.insert(object, position, simulation, resources);

    (id, object_id)
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
            type Obj = $crate::simulation::ChipId;
            fn make_game_object(
                id: $crate::simulation::ChipId,
                _args: (),
            ) -> $crate::simulation::ChipId {
                id
            }
        }
    };

    ($type:ty as $obj:ty) => {
        impl $crate::game_objects::MakeGameObject for $type {
            type Args = ();
            type Obj = $obj;
            fn make_game_object(id: $crate::simulation::ChipId, _args: ()) -> Self::Obj {
                <$obj as From::<_>>::from(id)
            }
        }
    };

    ($type:ty as $obj:ty where Args = $($args:ty),*) => {
        impl $crate::game_objects::MakeGameObject for $type {
            #[allow(unused_parens)]
            type Args = ($($args),*);
            type Obj = $obj;
            #[allow(unused_parens)]
            fn make_game_object(
                id: $crate::simulation::ChipId,
                args: ($($args),*),
            ) -> Self::Obj {
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
    pub fn id(&self) -> ObjectId {
        self.id
    }

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

    pub fn get_resource_mut<T: Resource>(&mut self) -> Option<&mut T> {
        self.resources.get_mut()
    }

    pub fn resource_mut<T: Resource>(&mut self) -> &mut T {
        self.get_resource_mut().unwrap()
    }

    pub fn get_resource<T: Resource>(&self) -> Option<&T> {
        self.resources.get()
    }

    pub fn resource<T: Resource>(&self) -> &T {
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
    resources: &'a TypeMap,
    id: ObjectId,
}

impl<'a> ObjectContext<'a> {
    pub fn new(state: &'a GameObjectState, id: ObjectId, resources: &'a TypeMap) -> Self {
        Self {
            state,
            id,
            resources,
        }
    }
}

impl ObjectContext<'_> {
    pub fn id(&self) -> ObjectId {
        self.id
    }

    pub fn resource<T: 'static>(&self) -> &T {
        self.resources.get::<T>().unwrap()
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

struct SetLayer(pub ObjectId, pub usize);

impl ObjectCommand for SetLayer {
    fn apply(&mut self, objects: &mut GameObjects, _: &mut Simulation, _: &mut Resources) {
        objects.draw_layers.set_layer(self.0, self.1);
    }
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
        self.hierarchy.insert_root(id);

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

    pub fn update_placement_ui(
        &mut self,
        simulation: &mut Simulation,
        resources: &mut Resources,
    ) -> UiInputResult {
        let mouse_screen = {
            let (x, y) = input::mouse_position();
            vec2(x, y)
        };
        let mouse_world = resources
            .get::<Camera>()
            .map(|camera| camera.get_mouse_world_pos())
            .unwrap_or(mouse_screen);

        let layout = placement_menu_layout(screen_width(), screen_height(), CHIP_CATALOG.len());
        let pointer_over_menu = layout.panel.contains(mouse_screen);
        let hovered = hit_test_menu_item(&layout.items, mouse_screen);

        let mut selected = resources
            .get::<PlacementUiState>()
            .and_then(|state| state.selected);

        if let Some(index) = hotkey_to_catalog_index().filter(|index| *index < CHIP_CATALOG.len()) {
            selected = Some(CHIP_CATALOG[index]);
        }

        if input::is_key_pressed(KeyCode::Escape)
            || input::is_mouse_button_pressed(MouseButton::Right)
        {
            selected = None;
        }

        let mut result = UiInputResult {
            pointer_over_menu,
            ..Default::default()
        };

        if pointer_over_menu && input::is_mouse_button_released(MouseButton::Left) {
            result.consume_world_left_release = true;
        }

        if input::is_mouse_button_pressed(MouseButton::Left) {
            if pointer_over_menu {
                result.consume_world_left_click = true;
                if let Some(index) = hovered {
                    selected = Some(CHIP_CATALOG[index]);
                }
            } else if let Some(template) = selected {
                result.consume_world_left_click = true;
                let origin = placement_origin_from_cursor(template, mouse_world);
                template.spawn_at(origin, simulation, self, resources);

                // deselect template unless user specifically wants to place multiple chips
                if !input::is_key_down(KeyCode::LeftShift) {
                    selected = None;
                }
            }
        }

        let ghost_world_pos =
            selected.map(|template| placement_origin_from_cursor(template, mouse_world));

        let ui_state = resources.get_mut_or_insert_default::<PlacementUiState>();
        ui_state.selected = selected;
        ui_state.hovered = hovered;
        ui_state.pointer_over_menu = pointer_over_menu;
        ui_state.menu_rect = Some(layout.panel);
        ui_state.item_rects = layout.items;
        ui_state.ghost_world_pos = ghost_world_pos;

        result
    }

    pub fn render_placement_overlays(&self, _simulation: &Simulation, resources: &TypeMap) {
        if let Some(ui_state) = resources.get::<PlacementUiState>()
            && let (Some(template), Some(position)) = (ui_state.selected, ui_state.ghost_world_pos)
        {
            self.draw_chip_preview(
                template,
                position,
                TILE_SIZE,
                Color::new(0.2, 0.2, 0.2, 0.45),
                Color::new(1.0, 1.0, 1.0, 0.8),
                Color::new(1.0, 0.3, 0.3, 0.9),
            );
        }

        set_default_camera();

        let fallback_layout =
            placement_menu_layout(screen_width(), screen_height(), CHIP_CATALOG.len());
        let (panel, item_rects, selected, hovered) =
            if let Some(state) = resources.get::<PlacementUiState>() {
                let rects = if state.item_rects.len() == CHIP_CATALOG.len() {
                    state.item_rects.clone()
                } else {
                    fallback_layout.items.clone()
                };
                (
                    state.menu_rect.unwrap_or(fallback_layout.panel),
                    rects,
                    state.selected,
                    state.hovered,
                )
            } else {
                (fallback_layout.panel, fallback_layout.items, None, None)
            };

        draw_rectangle(
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            Color::from_rgba(18, 26, 38, 220),
        );
        draw_rectangle_lines(
            panel.x,
            panel.y,
            panel.w,
            panel.h,
            2.0,
            Color::from_rgba(98, 132, 171, 220),
        );

        draw_text_ex(
            "Chip Palette",
            panel.x + 12.0,
            panel.y + 22.0,
            TextParams {
                font_size: 24,
                color: WHITE,
                ..Default::default()
            },
        );

        for (index, template) in CHIP_CATALOG.iter().copied().enumerate() {
            let rect = item_rects[index];
            let mut bg = Color::from_rgba(35, 48, 66, 220);

            if Some(index) == hovered {
                bg = Color::from_rgba(50, 68, 94, 245);
            }
            if Some(template) == selected {
                bg = Color::from_rgba(80, 98, 120, 245);
            }

            draw_rectangle(rect.x, rect.y, rect.w, rect.h, bg);
            draw_rectangle_lines(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                1.0,
                Color::from_rgba(120, 145, 174, 220),
            );

            let preview_box = Rect::new(rect.x + 8.0, rect.y + 6.0, 62.0, rect.h - 12.0);
            let geometry = template.preview_geometry();
            let scale = (preview_box.w / geometry.size_tiles.x as f32)
                .min(preview_box.h / geometry.size_tiles.y as f32)
                .max(1.0);
            let preview_size = geometry.size_tiles.as_vec2() * scale;
            let preview_pos = vec2(
                preview_box.x + (preview_box.w - preview_size.x) / 2.0,
                preview_box.y + (preview_box.h - preview_size.y) / 2.0,
            );

            self.draw_chip_preview(
                template,
                preview_pos,
                scale,
                DARKGRAY,
                LIGHTGRAY,
                Color::from_rgba(223, 85, 85, 240),
            );

            draw_text(template.label(), rect.x + 80.0, rect.y + 24.0, 22.0, WHITE);
            draw_text(
                &format!("[{}]", menu_hotkey_label(index)),
                rect.x + 80.0,
                rect.y + 44.0,
                18.0,
                GRAY,
            );
        }

        draw_text(
            "LMB: select/place  RMB/Esc: cancel",
            panel.x + 12.0,
            panel.y + panel.h - 8.0,
            18.0,
            LIGHTGRAY,
        );
    }

    fn draw_chip_preview(
        &self,
        template: ChipTemplate,
        position: Vec2,
        scale: f32,
        body_color: Color,
        border_color: Color,
        pin_color: Color,
    ) {
        let geometry = template.preview_geometry();
        let size = geometry.size_tiles.as_vec2() * scale;

        draw_rectangle(position.x, position.y, size.x, size.y, body_color);
        draw_rectangle_lines(
            position.x,
            position.y,
            size.x,
            size.y,
            (scale * 0.08).max(1.0),
            border_color,
        );

        let pin_radius = (scale * 0.15).max(1.5);
        for pin in geometry.pin_offsets_tiles {
            let pin_pos = position + pin * scale;
            draw_circle(pin_pos.x, pin_pos.y, pin_radius, pin_color);
            draw_circle_lines(pin_pos.x, pin_pos.y, pin_radius, 1.0, BLACK);
        }
    }

    fn find_by_hash(&mut self, type_id: TypeId, hash: u64) -> Option<ObjectId> {
        self.objects
            .iter()
            .find(|object| object.identifier == (type_id, hash))
            .map(|o| o.id)
    }

    pub fn find_and_despawn<O: GameObject + Hash>(&mut self, object: &O) {
        let mut hasher = DefaultHasher::default();
        object.hash(&mut hasher);
        let hash = hasher.finish();
        if let Some(id) = self.find_by_hash(TypeId::of::<O>(), hash) {
            self.despawn(id);
        }
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
        if let Some(children) = self.hierarchy.remove_recursively(id) {
            for child in children {
                self.despawn(child);
            }
        }
        self.draw_layers.remove(id);
    }

    pub fn render(&self, simulation: &Simulation, resources: &TypeMap) {
        for (id, object, state) in self.draw_layers.iter_ordered().map(|id| {
            (
                id,
                &*self.objects.get(id.0).unwrap().object,
                self.state.get(id.0).unwrap(),
            )
        }) {
            object.render(&ObjectContext::new(state, id, resources), simulation, self)
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

    pub fn get_overlapping_by_type<T: GameObject + Hash>(
        &self,
        shape: Shape,
    ) -> impl Iterator<Item = ObjectId> {
        let type_id = TypeId::of::<T>();
        self.objects
            .iter()
            .zip(self.state.iter())
            .filter(move |(object, state)| {
                object.identifier.0 == type_id
                    && state
                        .shape
                        .map(|object_shape| object_shape.overlaps(&shape))
                        .unwrap_or(false)
            })
            .map(|(object, _)| object.id)
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

#[derive(Clone, Copy)]
pub enum Shape {
    Rectangle(Rect),
    Circle(Circle),
}

impl Shape {
    pub fn rect_corners(top_left: Vec2, bottom_right: Vec2) -> Self {
        let dim = bottom_right - top_left;
        Self::Rectangle(Rect::new(top_left.x, top_left.y, dim.x, dim.y))
    }

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

    pub fn overlaps(&self, other: &Shape) -> bool {
        match (*self, *other) {
            (Self::Rectangle(a), Self::Rectangle(b)) => a.overlaps(&b),
            (Self::Rectangle(rect), Self::Circle(circle))
            | (Self::Circle(circle), Self::Rectangle(rect)) => circle.overlaps_rect(&rect),
            (Self::Circle(a), Self::Circle(b)) => a.overlaps(&b),
        }
    }
}

pub struct GameObjectState {
    pub position: Vec2,
    pub shape: Option<Shape>,
    pub hovered: bool,
    pub custom_data: TypeMap,
}

// TODO: destroy hook.
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

pub(super) trait PlaceMgos<T, const N: usize> {
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)>;
}

impl<C: MakeGameObject + 'static, MGO: SplitForMgo<C>> PlaceMgos<C, 1> for MGO {
    fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
        let (c, pos, args) = self.split_for_mgo();
        vec![game.place_chip(c, pos.as_vec2() * TILE_SIZE, args)]
    }
}

macro_rules! impl_place_mgos {
    ($count:literal; $(($chip:ident, $mgo:ident, $index:tt)),+ $(,)?) => {
        impl<$($chip: MakeGameObject + 'static, $mgo: SplitForMgo<$chip>),+>
            PlaceMgos<($($chip),+), $count> for ($($mgo),+)
        {
            fn place(self, game: &mut Game) -> Vec<(ChipId, ObjectId)> {
                let mut out = Vec::with_capacity($count);
                $(
                    let (c, pos, args) = self.$index.split_for_mgo();
                    out.push(game.place_chip(c, pos.as_vec2() * TILE_SIZE, args));
                )+
                out
            }
        }
    };
}

impl_place_mgos!(2; (C0, MGO0, 0), (C1, MGO1, 1));
impl_place_mgos!(3; (C0, MGO0, 0), (C1, MGO1, 1), (C2, MGO2, 2));
impl_place_mgos!(
    4;
    (C0, MGO0, 0),
    (C1, MGO1, 1),
    (C2, MGO2, 2),
    (C3, MGO3, 3)
);
impl_place_mgos!(
    5;
    (C0, MGO0, 0),
    (C1, MGO1, 1),
    (C2, MGO2, 2),
    (C3, MGO3, 3),
    (C4, MGO4, 4)
);
impl_place_mgos!(
    6;
    (C0, MGO0, 0),
    (C1, MGO1, 1),
    (C2, MGO2, 2),
    (C3, MGO3, 3),
    (C4, MGO4, 4),
    (C5, MGO5, 5)
);
impl_place_mgos!(
    7;
    (C0, MGO0, 0),
    (C1, MGO1, 1),
    (C2, MGO2, 2),
    (C3, MGO3, 3),
    (C4, MGO4, 4),
    (C5, MGO5, 5),
    (C6, MGO6, 6)
);
impl_place_mgos!(
    8;
    (C0, MGO0, 0),
    (C1, MGO1, 1),
    (C2, MGO2, 2),
    (C3, MGO3, 3),
    (C4, MGO4, 4),
    (C5, MGO5, 5),
    (C6, MGO6, 6),
    (C7, MGO7, 7)
);
