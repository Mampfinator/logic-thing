use macroquad::{
    input::{self, KeyCode},
    math::{Rect, UVec2, Vec2, vec2},
    prelude::RED,
};

use crate::{
    Clock, Counter8b, Led, Nand, NumericDisplay, Resource, Resources, TILE_SIZE, TieHigh,
    chips::{button, cpu::CPU, rom, switch::Switches},
    simulation::{Chip, ChipId, Simulation},
};

use super::{GameObjects, ObjectId, spawn_make_object};

pub(super) const CHIP_CATALOG: [ChipTemplate; 10] = [
    ChipTemplate::Cpu,
    ChipTemplate::Rom,
    ChipTemplate::TieHigh,
    ChipTemplate::Button,
    ChipTemplate::Switches,
    ChipTemplate::NumericDisplay,
    ChipTemplate::Clock,
    ChipTemplate::Counter8b,
    ChipTemplate::Led,
    ChipTemplate::Nand,
];

const MENU_PANEL_MARGIN: f32 = 12.0;
const MENU_PANEL_WIDTH: f32 = 280.0;
const MENU_HEADER_HEIGHT: f32 = 28.0;
const MENU_FOOTER_HEIGHT: f32 = 24.0;
const MENU_ITEM_HEIGHT: f32 = 52.0;
const MENU_ITEM_GAP: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChipTemplate {
    Cpu,
    Rom,
    TieHigh,
    Button,
    Switches,
    NumericDisplay,
    Clock,
    Counter8b,
    Led,
    Nand,
}

#[derive(Default)]
pub struct PlacementUiState {
    pub selected: Option<ChipTemplate>,
    pub hovered: Option<usize>,
    pub pointer_over_menu: bool,
    pub menu_rect: Option<Rect>,
    pub item_rects: Vec<Rect>,
    pub ghost_world_pos: Option<Vec2>,
}

impl Resource for PlacementUiState {}

#[derive(Default, Clone, Copy)]
pub struct UiInputResult {
    pub consume_world_left_click: bool,
    pub consume_world_left_release: bool,
    pub pointer_over_menu: bool,
}

#[derive(Clone)]
pub(super) struct MenuLayout {
    pub(super) panel: Rect,
    pub(super) items: Vec<Rect>,
}

pub(super) struct ChipPreviewGeometry {
    pub(super) size_tiles: UVec2,
    pub(super) pin_offsets_tiles: Vec<Vec2>,
}

pub(super) fn placement_menu_layout(screen_w: f32, screen_h: f32, item_count: usize) -> MenuLayout {
    let item_count = item_count as f32;
    let panel_height = MENU_PANEL_MARGIN * 2.0
        + MENU_HEADER_HEIGHT
        + MENU_FOOTER_HEIGHT
        + item_count * MENU_ITEM_HEIGHT
        + (item_count - 1.0).max(0.0) * MENU_ITEM_GAP;
    let panel = Rect::new(
        screen_w - MENU_PANEL_WIDTH - MENU_PANEL_MARGIN,
        MENU_PANEL_MARGIN,
        MENU_PANEL_WIDTH,
        panel_height.min(screen_h - MENU_PANEL_MARGIN * 2.0),
    );

    let mut items = Vec::with_capacity(item_count as usize);
    let mut y = panel.y + MENU_PANEL_MARGIN + MENU_HEADER_HEIGHT;
    for _ in 0..item_count as usize {
        items.push(Rect::new(
            panel.x + MENU_PANEL_MARGIN,
            y,
            panel.w - MENU_PANEL_MARGIN * 2.0,
            MENU_ITEM_HEIGHT,
        ));
        y += MENU_ITEM_HEIGHT + MENU_ITEM_GAP;
    }

    MenuLayout { panel, items }
}

pub(super) fn hit_test_menu_item(item_rects: &[Rect], mouse_screen: Vec2) -> Option<usize> {
    item_rects
        .iter()
        .position(|rect| rect.contains(mouse_screen))
}

fn keycode_to_catalog_index(key: KeyCode) -> Option<usize> {
    match key {
        KeyCode::Key1 => Some(0),
        KeyCode::Key2 => Some(1),
        KeyCode::Key3 => Some(2),
        KeyCode::Key4 => Some(3),
        KeyCode::Key5 => Some(4),
        KeyCode::Key6 => Some(5),
        KeyCode::Key7 => Some(6),
        KeyCode::Key8 => Some(7),
        KeyCode::Key9 => Some(8),
        KeyCode::Key0 => Some(9),
        _ => None,
    }
}

pub(super) fn hotkey_to_catalog_index() -> Option<usize> {
    for key in [
        KeyCode::Key1,
        KeyCode::Key2,
        KeyCode::Key3,
        KeyCode::Key4,
        KeyCode::Key5,
        KeyCode::Key6,
        KeyCode::Key7,
        KeyCode::Key8,
        KeyCode::Key9,
        KeyCode::Key0,
    ] {
        if input::is_key_pressed(key) {
            return keycode_to_catalog_index(key);
        }
    }

    None
}

pub(super) fn menu_hotkey_label(index: usize) -> &'static str {
    match index {
        0 => "1",
        1 => "2",
        2 => "3",
        3 => "4",
        4 => "5",
        5 => "6",
        6 => "7",
        7 => "8",
        8 => "9",
        9 => "0",
        _ => "?",
    }
}

pub(super) fn default_rom_bytes() -> [u8; 256] {
    let mut rom = [0_u8; 256];
    for i in 0..=u8::MAX {
        rom[i as usize] = i;
    }
    rom
}

fn snap_to_grid(position: Vec2, spacing: f32) -> Vec2 {
    vec2(
        (position.x / spacing).round() * spacing,
        (position.y / spacing).round() * spacing,
    )
}

pub(super) fn placement_origin_from_cursor(template: ChipTemplate, cursor_world_pos: Vec2) -> Vec2 {
    let size_pixels = template.tile_size().as_vec2() * TILE_SIZE;
    let centered_origin = cursor_world_pos - size_pixels / 2.0;
    snap_to_grid(centered_origin, TILE_SIZE)
}

impl ChipTemplate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Rom => "ROM",
            Self::TieHigh => "Tie High",
            Self::Button => "Button",
            Self::Switches => "Switches",
            Self::NumericDisplay => "Numeric Display",
            Self::Clock => "Clock",
            Self::Counter8b => "Counter 8-bit",
            Self::Led => "LED",
            Self::Nand => "NAND",
        }
    }

    pub fn tile_size(self) -> UVec2 {
        match self {
            Self::Cpu => UVec2::new(4, 14),
            Self::Rom => UVec2::new(2, 9),
            Self::TieHigh => UVec2::new(1, 1),
            Self::Button => UVec2::new(1, 1),
            Self::Switches => UVec2::new(4, 8),
            Self::NumericDisplay => UVec2::new(8, 4),
            Self::Clock => UVec2::new(1, 2),
            Self::Counter8b => UVec2::new(2, 8),
            Self::Led => UVec2::new(1, 1),
            Self::Nand => UVec2::new(2, 4),
        }
    }

    pub fn spawn_at(
        self,
        position: Vec2,
        simulation: &mut Simulation,
        game_objects: &mut GameObjects,
        resources: &mut Resources,
    ) -> (ChipId, ObjectId) {
        match self {
            Self::Cpu => spawn_make_object(
                simulation,
                game_objects,
                resources,
                CPU::default(),
                position,
                (),
            ),
            Self::Rom => spawn_make_object(
                simulation,
                game_objects,
                resources,
                rom::ROM::from(default_rom_bytes()),
                position,
                (),
            ),
            Self::TieHigh => {
                spawn_make_object(simulation, game_objects, resources, TieHigh, position, ())
            }
            Self::Button => spawn_make_object(
                simulation,
                game_objects,
                resources,
                button::Button,
                position,
                (),
            ),
            Self::Switches => spawn_make_object(
                simulation,
                game_objects,
                resources,
                Switches::new(8),
                position,
                8usize,
            ),
            Self::NumericDisplay => spawn_make_object(
                simulation,
                game_objects,
                resources,
                NumericDisplay,
                position,
                (),
            ),
            Self::Clock => spawn_make_object(
                simulation,
                game_objects,
                resources,
                Clock::new(30),
                position,
                (),
            ),
            Self::Counter8b => spawn_make_object(
                simulation,
                game_objects,
                resources,
                Counter8b::default(),
                position,
                (),
            ),
            Self::Led => spawn_make_object(simulation, game_objects, resources, Led, position, RED),
            Self::Nand => spawn_make_object(
                simulation,
                game_objects,
                resources,
                Nand::new(2),
                position,
                (),
            ),
        }
    }

    pub(super) fn preview_geometry(self) -> ChipPreviewGeometry {
        fn build_for<C: Chip + 'static>(chip: C) -> ChipPreviewGeometry {
            let mut simulation = Simulation::default();
            let chip_id = simulation.place_chip(chip);
            let instance = simulation.chips.get(chip_id).unwrap();

            ChipPreviewGeometry {
                size_tiles: instance.size,
                pin_offsets_tiles: instance
                    .pins_as_positions()
                    .map(|(pin, _)| pin.get_pin_tile_offset(instance.size) / TILE_SIZE)
                    .collect::<Vec<_>>(),
            }
        }

        match self {
            Self::Cpu => build_for(CPU::default()),
            Self::Rom => build_for(rom::ROM::from(default_rom_bytes())),
            Self::TieHigh => build_for(TieHigh),
            Self::Button => build_for(button::Button),
            Self::Switches => build_for(Switches::new(8)),
            Self::NumericDisplay => build_for(NumericDisplay),
            Self::Clock => build_for(Clock::new(30)),
            Self::Counter8b => build_for(Counter8b::default()),
            Self::Led => build_for(Led),
            Self::Nand => build_for(Nand::new(2)),
        }
    }
}

#[cfg(test)]
mod placement_ui_tests {
    use macroquad::math::vec2;

    use super::*;

    #[test]
    fn chip_template_metadata_is_non_empty_and_sized() {
        for template in CHIP_CATALOG {
            assert!(!template.label().is_empty());
            let size = template.tile_size();
            assert!(size.x > 0);
            assert!(size.y > 0);
        }
    }

    #[test]
    fn menu_hit_test_and_hotkey_mapping() {
        let layout = placement_menu_layout(1280.0, 720.0, CHIP_CATALOG.len());
        let first = layout.items[0];
        let point = vec2(first.x + first.w * 0.5, first.y + first.h * 0.5);
        assert_eq!(hit_test_menu_item(&layout.items, point), Some(0));
        assert_eq!(keycode_to_catalog_index(KeyCode::Key1), Some(0));
        assert_eq!(keycode_to_catalog_index(KeyCode::Key0), Some(9));
        assert_eq!(keycode_to_catalog_index(KeyCode::A), None);
    }

    #[test]
    fn default_rom_is_identity() {
        let rom = default_rom_bytes();
        for i in 0..=u8::MAX {
            assert_eq!(rom[i as usize], i);
        }
    }
}
