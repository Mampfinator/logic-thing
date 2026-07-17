use std::fs::read_to_string;

use macroquad::{input, prelude::*};

use crate::{
    chips::programmable::ProgrammableChip, game::{FullscreenState, OldSimulationSpeed, SimulationControl}, game_objects::{
        Grid,
        simulation_types::{HoveredPins, PinObjectIds, Selection, pin_label},
    }, loader::load_chips,
};

pub const TILE_SIZE: f32 = 16.0;

pub mod chips;
pub mod game;
pub mod game_objects;
pub mod loader;
pub mod simulation;

// Keep the crate-level API stable while implementations live with their domains.
pub use game::{Game, Resource};
pub use game_objects::{GameObject, GameObjects, GetState, ObjectContext, ObjectContextMut};

#[macroquad::main("Chip Game")]
async fn main() {
    request_new_screen_size(1080., 720.);
    next_frame().await;

    let mut game = Game::default();
    game.resources
        .insert_default::<SimulationControl>()
        .insert_default::<Selection>()
        .insert_default::<HoveredPins>()
        .insert_default::<FullscreenState>()
        .insert_default::<PinObjectIds>();

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

    game = load_chips(game, read_to_string("example.rhai").unwrap());
    game.place_chips((
        ProgrammableChip::from_code(include_str!("../chip-test.rhai")).unwrap(),
        ivec2(20, 20),
    ));

    for _ in 0.. {
        clear_background(SKYBLUE);
        
        if game.resources.get_mut::<SimulationControl>().unwrap().check() {
            game.simulation.tick();
        }

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

        if input::is_key_down(KeyCode::LeftControl) && input::is_key_pressed(KeyCode::Space) {
            let old_speed = game.resources.delete::<OldSimulationSpeed>();
            let current_state = game.resources.get_mut::<SimulationControl>().unwrap();

            match (current_state.get_resolution(), old_speed) {
                // simulation is paused. We resume with old speed.
                (None, Some(old_speed)) => {
                    current_state.set_resolution(old_speed.0)
                },
                // simulation is paused but we don't have an old speed. Just go with the default one.
                (None, None) => {
                    current_state.set_resolution(1. / 60.);
                },
                (Some(current), _) => {
                    current_state.stop();
                    game.resources.insert(OldSimulationSpeed(current));
                }
            }
        }

        if input::is_key_pressed(KeyCode::Space) && game.resources.get::<SimulationControl>().unwrap().is_paused() {
            game.simulation.tick();
        }

        let selection = game.resources.get_mut::<Selection>().unwrap();
        if input::is_mouse_button_pressed(MouseButton::Right) {
            selection.reset();
        }

        set_default_camera();
        if let Some(text) = match selection {
            Selection::Chip(id) => Some(format!("Chip selected: {id:?}")),
            Selection::Pin(id) => Some(format!(
                "Pin selected: {}",
                pin_label(&game.simulation, *id)
            )),
            Selection::MultiPins(pins) => Some(format!(
                "Pins selected: {}",
                pins.iter()
                    .copied()
                    .map(|pin| pin_label(&game.simulation, pin))
                    .collect::<Vec<_>>()
                    .join(","),
            )),
            Selection::None => None,
        } {
            draw_text(&text, 0., 100., 32., WHITE);
        }

        game.render();
        draw_fps();
        next_frame().await;
    }
}
