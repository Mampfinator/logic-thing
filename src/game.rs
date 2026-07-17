use std::{
    any::{Any, TypeId},
    collections::HashMap,
};

use macroquad::{
    camera::{Camera2D, set_camera},
    input::{self, KeyCode, MouseButton},
    math::{Vec2, vec2},
    prelude::{screen_height, screen_width, set_fullscreen},
};

use crate::{
    game_objects::{
        CommandBuffer, GameObjects, GetState, MakeGameObject, ObjectContextMut, ObjectId,
        PlaceMgos, Shape, TypeMap,
        simulation_types::{DragSelectionStart, PinObjectIds, Selection},
        spawn_make_object,
    },
    simulation::{ChipId, PinId, Simulation},
};

#[derive(Default, Debug)]
pub struct Game {
    pub simulation: Simulation,
    pub game_objects: GameObjects,
    pub resources: Resources,
}

pub struct Camera {
    pub(crate) camera: Camera2D,
    zoom_factor: f32,
}

impl Resource for Camera {
    fn update(&mut self, _: &mut TypeMap, _: &mut GameCommands) {
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
            self.camera.target += input::mouse_delta_position() * 1000. / self.zoom_factor;
        } else {
            let x = if input::is_key_down(KeyCode::A) {
                -1.
            } else if input::is_key_down(KeyCode::D) {
                1.
            } else {
                0.
            };
            let y = if input::is_key_down(KeyCode::W) {
                -1.
            } else if input::is_key_down(KeyCode::S) {
                1.
            } else {
                0.
            };
            if x != 0. || y != 0. {
                self.camera.target += vec2(x, y).normalize() * 5.;
            }
        }

        set_camera(&self.camera);
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            camera: Camera2D {
                target: vec2(screen_width() / 2., screen_height() / 2.),
                ..Default::default()
            },
            zoom_factor: 1.5,
        }
    }
}

impl Camera {
    pub fn get_mouse_world_pos(&self) -> Vec2 {
        let (x, y) = input::mouse_position();
        self.camera.screen_to_world(vec2(x, y))
    }

    fn zoom_by(&mut self, by: f32) {
        self.zoom_factor = (self.zoom_factor + by).max(0.1);
    }
}

#[derive(Default)]
pub struct FullscreenState(bool);

impl Resource for FullscreenState {
    fn update(&mut self, _: &mut TypeMap, _: &mut GameCommands) {
        if input::is_key_pressed(KeyCode::F) {
            self.0 = !self.0;
            set_fullscreen(self.0);
        }
    }
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
    #[allow(private_bounds, reason = "PlaceMgos is just a collection trait.")]
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
        <C as MakeGameObject>::Obj: std::hash::Hash,
    {
        spawn_make_object(
            &mut self.simulation,
            &mut self.game_objects,
            &mut self.resources,
            chip,
            position,
            args,
        )
    }

    pub fn delete_chip(&mut self, chip: ChipId) {
        self.simulation.remove_chip(chip);
        self.game_objects.find_and_despawn(&chip);
    }

    fn camera(&mut self) -> &mut Camera {
        self.resources.get_mut_or_insert_default()
    }

    pub fn update(&mut self) {
        let mut commands = GameCommands::default();
        self.resources.update(&mut commands);
        commands.buffer.apply(self);

        // process mouse information.
        let mouse_pos = self.camera().get_mouse_world_pos();
        let clicked = input::is_mouse_button_pressed(MouseButton::Left);
        let released = input::is_mouse_button_released(MouseButton::Left);
        let ui_result = self
            .game_objects
            .update_placement_ui(&mut self.simulation, &mut self.resources);
        let clicked = clicked && !ui_result.consume_world_left_click;
        let released = released && !ui_result.consume_world_left_release;

        let mut buffer = CommandBuffer::default();

        let mut any_on_click_triggered = false;

        for (id, object, state) in self.game_objects.iter_mut() {
            let mut ctx = ObjectContextMut::new(state, id, &mut buffer, &mut self.resources);
            if ui_result.pointer_over_menu {
                if ctx.hovered() {
                    ctx.set_hovered(false);
                    object.on_mouse_exit(&mut ctx, &mut self.simulation);
                }
                continue;
            }

            let Some(is_inside) = ctx
                .get_state()
                .shape
                .as_ref()
                .map(|shape| shape.contains(mouse_pos))
            else {
                continue;
            };

            if is_inside && !ctx.hovered() {
                ctx.set_hovered(true);
                object.on_mouse_enter(&mut ctx, &mut self.simulation);
            }

            if !is_inside && ctx.hovered() {
                ctx.set_hovered(false);
                object.on_mouse_exit(&mut ctx, &mut self.simulation);
            }

            if is_inside && clicked {
                any_on_click_triggered = true;
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

        // special logic for multi-pin selection, since otherwise that'd just be duplicate click tracking work.
        if clicked && !any_on_click_triggered {
            self.resources.insert(DragSelectionStart(mouse_pos));
        }

        if released
            && let Some(DragSelectionStart(start)) = self.resources.delete::<DragSelectionStart>()
        {
            let shape = Shape::rect_corners(start, mouse_pos);
            let pin_ids = self.resources.get::<PinObjectIds>().unwrap();
            let pins = self
                .game_objects
                .get_overlapping_by_type::<PinId>(shape)
                .filter_map(|object| pin_ids.0.get(&object))
                .copied()
                .collect::<Vec<_>>();

            if !pins.is_empty() {
                self.resources.insert(Selection::MultiPins(pins));
            } else {
                self.resources.insert(Selection::None);
            }
        }

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

    pub fn render(&mut self) {
        set_camera(&self.camera().camera);
        self.resources.render();
        self.game_objects
            .render(&self.simulation, self.resources.as_typemap());
        self.game_objects
            .render_placement_overlays(&self.simulation, self.resources.as_typemap());
    }
}

pub trait Resource: Any + 'static {
    #[allow(unused)]
    fn update(&mut self, resources: &mut TypeMap, commands: &mut GameCommands) {}
    #[allow(unused)]
    fn render(&mut self, resources: &mut TypeMap) {}
}

impl Resources {
    pub fn update(&mut self, commands: &mut GameCommands) {
        for table in self.vtables.values() {
            (table.update)(&mut self.resources, commands);
        }
    }

    pub fn render(&mut self) {
        for table in self.vtables.values() {
            (table.render)(&mut self.resources)
        }
    }

    pub fn insert<T: Resource>(&mut self, value: T) -> &mut Self {
        self.resources.insert(value);
        self.vtables
            .insert(TypeId::of::<T>(), ResourceVTable::of::<T>());
        self
    }

    pub fn get_mut<T: Resource>(&mut self) -> Option<&mut T> {
        self.resources.get_mut()
    }

    pub fn get<T: Resource>(&self) -> Option<&T> {
        self.resources.get()
    }

    pub fn insert_default<T: Resource + Default>(&mut self) -> &mut Self {
        self.insert(T::default())
    }

    pub fn get_mut_or_insert_default<T: Resource + Default>(&mut self) -> &mut T {
        if !self.vtables.contains_key(&TypeId::of::<T>()) {
            self.insert(T::default());
        }

        self.resources.get_mut().unwrap()
    }

    pub fn delete<T: Resource>(&mut self) -> Option<T> {
        let removed = self.resources.delete();
        if removed.is_some() {
            self.vtables.remove(&TypeId::of::<T>());
        }

        removed
    }

    pub fn resource_scope<T: Resource, F: FnOnce(&mut T, &mut Self)>(&mut self, f: F) -> &mut Self {
        let Some(mut resource) = self.resources.delete::<T>() else {
            return self;
        };

        f(&mut resource, self);

        self.resources.insert(resource);

        self
    }

    pub fn as_typemap(&self) -> &TypeMap {
        &self.resources
    }
}

struct ResourceVTable {
    pub update: fn(&mut TypeMap, &mut GameCommands),
    pub render: fn(&mut TypeMap),
}

impl ResourceVTable {
    pub const fn of<T: Resource>() -> Self {
        Self {
            update: |types, commands| {
                types.scoped(|value: &mut T, types| {
                    value.update(types, commands);
                })
            },
            render: |types| {
                types.scoped(|value: &mut T, types| {
                    value.render(types);
                })
            },
        }
    }
}

#[derive(Default)]
pub struct Resources {
    resources: TypeMap,
    vtables: HashMap<TypeId, ResourceVTable>,
}

impl std::fmt::Debug for Resources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Resources(#{})", self.vtables.len())
    }
}

trait GameCommand {
    fn apply(&mut self, game: &mut Game);
}

struct RemoveChip(ChipId);
impl GameCommand for RemoveChip {
    fn apply(&mut self, game: &mut Game) {
        game.delete_chip(self.0);
    }
}

#[derive(Default)]
struct GameCommandBuffer(Vec<Box<dyn GameCommand>>);
impl GameCommandBuffer {
    pub fn apply(self, game: &mut Game) {
        for mut command in self.0 {
            command.apply(game)
        }
    }
}

#[derive(Default)]
pub struct GameCommands {
    buffer: GameCommandBuffer,
}

impl GameCommands {
    pub fn remove_chip(&mut self, chip: ChipId) -> &mut Self {
        self.buffer.0.push(Box::new(RemoveChip(chip)));
        self
    }
}
