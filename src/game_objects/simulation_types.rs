use std::collections::{HashMap, HashSet};

use macroquad::{
    color::{BLACK, BLUE, Color, DARKBLUE, DARKGRAY, LIGHTGRAY, RED, WHITE},
    input::{self, KeyCode},
    math::{Circle, Rect, Vec2, vec2},
    shapes::{draw_circle, draw_circle_lines, draw_line, draw_rectangle, draw_rectangle_lines},
    text::{TextParams, draw_text_ex},
};

use crate::{
    TILE_SIZE,
    game::{Camera, GameCommands, Resource},
    game_objects::{
        GameObject, GameObjects, GetState, ObjectContext, ObjectContextMut, ObjectId, Shape,
        TypeMap,
    },
    simulation::{ChipId, NetworkId, Pin, PinId, Simulation},
};

#[derive(Default)]
pub struct PinObjectIds(pub HashMap<ObjectId, PinId>);
impl Resource for PinObjectIds {}

pub struct DragSelectionStart(pub Vec2);
impl Resource for DragSelectionStart {
    fn render(&mut self, resources: &mut TypeMap) {
        let camera = resources.get::<Camera>().unwrap();
        let current = camera.get_mouse_world_pos();
        let size = current - self.0;
        draw_rectangle(self.0.x, self.0.y, size.x, size.y, DARKBLUE.with_alpha(0.2));
        draw_rectangle_lines(
            self.0.x,
            self.0.y,
            size.x,
            size.y,
            2.,
            DARKBLUE.with_alpha(0.5),
        );
    }
}

#[derive(Default)]
pub struct HoveredPins {
    pins: HashSet<PinId>,
}
impl Resource for HoveredPins {}

impl HoveredPins {
    pub fn clear(&mut self) {
        self.pins.clear();
    }
    pub fn remove_one(&mut self, pin: PinId) {
        self.pins.remove(&pin);
    }
    pub fn set<T: Iterator<Item = PinId>>(&mut self, pins: T) {
        self.pins.clear();
        self.pins.extend(pins);
    }
    pub fn contains_either(&self, a: PinId, b: PinId) -> bool {
        self.pins.contains(&a) || self.pins.contains(&b)
    }
}

#[derive(Default, Clone)]
pub enum Selection {
    #[default]
    None,
    Pin(PinId),
    Chip(ChipId),
    MultiPins(Vec<PinId>),
}

impl Resource for Selection {
    fn update(&mut self, _: &mut TypeMap, commands: &mut GameCommands) {
        if input::is_key_pressed(KeyCode::Delete)
            && let Self::Chip(chip) = self
        {
            commands.remove_chip(*chip);
            self.reset();
        }
    }
}

#[derive(Debug)]
pub enum PinSelection<'a> {
    One(PinId),
    Multiple(&'a [PinId]),
}

impl Selection {
    pub fn select_chip(&mut self, chip: ChipId) {
        *self = Self::Chip(chip);
    }
    pub fn select_pin(&mut self, pin: PinId) -> Option<PinSelection<'_>> {
        match self {
            Self::Pin(other) => Some(PinSelection::One(*other)),
            Self::MultiPins(others) => Some(PinSelection::Multiple(others)),
            _ => {
                *self = Self::Pin(pin);
                None
            }
        }
    }
    pub fn reset(&mut self) {
        *self = Self::None;
    }
}

struct PinLabelMeta(String, Vec2, f32);

#[derive(Default)]
struct ChipClickOffset(Vec2);

fn snap_to_grid(position: Vec2, spacing: f32) -> Vec2 {
    vec2(
        (position.x / spacing).round() * spacing,
        (position.y / spacing).round() * spacing,
    )
}

fn get_multi_pin_range_other(
    range: &[PinId],
    simulation: &Simulation,
    target: PinId,
) -> Option<Vec<PinId>> {
    let pin = simulation.pins.get(target)?;
    simulation
        .chips
        .get(pin.chip)?
        .get_pin_range(target, range.len())
}

pub fn pin_label(simulation: &Simulation, pin: PinId) -> String {
    let pin = simulation.pins.get(pin).unwrap();
    pin.label
        .clone()
        .unwrap_or_else(|| format!("Pin {}", pin.id.0))
}

impl GameObject for NetworkId {
    fn start(&mut self, state: &mut ObjectContextMut, _: &Simulation) {
        state.set_layer(0);
    }

    fn update(&mut self, state: &mut ObjectContextMut, simulation: &mut Simulation) {
        if simulation.networks.get(*self).is_none() {
            state.despawn();
        }
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        let highlight_pins = ctx.resource::<HoveredPins>();

        let Some(network) = simulation.networks.get(*self) else {
            return;
        };

        for (a, b) in network.iter_connections() {
            let pos_a = objects.find_state(&a).unwrap().position;
            let pos_b = objects.find_state(&b).unwrap().position;

            let alpha = if highlight_pins.contains_either(a, b) {
                0.9
            } else {
                0.5
            };

            let color = if network.state {
                Color::new(0.7, 0.1, 0.1, alpha)
            } else {
                Color::new(0.1, 0.7, 0.1, alpha)
            };

            let thickness = if highlight_pins.contains_either(a, b) {
                4.
            } else {
                2.
            };

            draw_line(pos_a.x, pos_a.y, pos_b.x, pos_b.y, thickness, color);
        }
    }
}

impl GameObject for PinId {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        let id = ctx.id();
        ctx.resource_mut::<PinObjectIds>().0.insert(id, *self);

        ctx.set_layer(2);

        ctx.set_shape(Shape::Circle(Circle {
            x: ctx.position().x,
            y: ctx.position().y,
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

            let text_length_offset = label.len() as f32 * 12.;

            let (text_offset, rotation) = match pin_side {
                Pin::Right(_) => (vec2(TILE_SIZE / 2., 0.), 0.),
                Pin::Bottom(_) => (vec2(0., TILE_SIZE / 2.), std::f32::consts::PI / 2.),
                Pin::Left(_) => (vec2(-TILE_SIZE / 2. - text_length_offset, 0.), 0.),
                Pin::Top(_) => (
                    vec2(0., -TILE_SIZE / 2. - text_length_offset),
                    std::f32::consts::PI / 2.,
                ),
            };

            // we can be *reasonably* sure that pin labels won't ever change. so this should be fine. probably.
            ctx.insert_data(PinLabelMeta(label, text_offset, rotation));
        };
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        let selection = ctx.resource_mut::<Selection>();
        if let Some(other) = selection.select_pin(*self) {
            match other {
                PinSelection::One(pin) => {
                    simulation.toggle_connect_by_pinid(pin, *self);
                    selection.reset();
                }
                PinSelection::Multiple(others) => {
                    // TODO: preview
                    if let Some(other_range) = get_multi_pin_range_other(others, simulation, *self)
                    {
                        for (a, b) in other_range.into_iter().zip(others.iter().copied()) {
                            simulation.connect(a, b);
                        }
                    }

                    selection.reset();
                }
            }
        }
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, _: &mut Simulation) {
        if ctx.hovered() {
            ctx.resource_mut::<HoveredPins>().set([*self].into_iter());
        }
    }

    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, _: &mut Simulation) {
        ctx.resource_mut::<HoveredPins>().remove_one(*self);
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        let on_state = simulation.pins.get_state(*self).unwrap();
        let position = ctx.position();

        let color = if on_state { RED } else { LIGHTGRAY };

        draw_circle(position.x, position.y, TILE_SIZE / 4., color);
        draw_circle_lines(position.x, position.y, TILE_SIZE / 4., 1., BLACK);

        if let Some(meta) = ctx.get_data::<PinLabelMeta>() {
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

        if ctx.hovered() {
            let selection = ctx.resource::<Selection>();
            if let Selection::MultiPins(others) = selection
                && let Some(other_range) = get_multi_pin_range_other(others, simulation, *self)
            {
                for (a, b) in others.iter().copied().zip(other_range) {
                    let pos_a = objects.find_state(&a).unwrap().position;
                    let pos_b = objects.find_state(&b).unwrap().position;

                    draw_line(pos_a.x, pos_a.y, pos_b.x, pos_b.y, 2., BLUE.with_alpha(0.7));
                }
            }
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
            ctx.resource_mut::<Selection>().select_chip(*self);
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

    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, _: &mut Simulation) {
        ctx.resource_mut::<HoveredPins>().clear();
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        if ctx.hovered() {
            let chip = simulation.chips.get(*self).unwrap();
            let hovered = ctx.resource_mut::<HoveredPins>();
            hovered.set(chip.pins.iter().filter_map(|p| *p))
        }

        if let Some(offset) = ctx.get_data::<ChipClickOffset>() {
            let mouse_pos = ctx.mouse_world_pos();
            ctx.move_by(mouse_pos - ctx.position() + offset.0);
        }
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, _: &GameObjects) {
        let instance = simulation.chips.get(*self).unwrap();
        let size = instance.size.as_vec2() * TILE_SIZE;
        let position = ctx.position();

        draw_rectangle(position.x, position.y, size.x, size.y, DARKGRAY);
        if ctx.hovered() {
            draw_rectangle_lines(
                position.x + 1.,
                position.y + 1.,
                size.x - 2.,
                size.y - 2.,
                2.,
                WHITE,
            );
        }

        draw_rectangle_lines(position.x, position.y, size.x, size.y, 1., BLACK);
    }
}
