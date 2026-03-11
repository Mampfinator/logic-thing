use std::{rc::Rc, sync::Mutex};

use macroquad::{
    color::Color,
    math::{IVec2, ivec2},
};
use rhai::Engine;

use crate::{
    Clock, Counter8b, Game, TieHigh,
    chips::cpu::CPU,
    simulation::{ChipId, Pin},
};

macro_rules! shadow {
    { [$($vars:ident),*] $closure:expr} => {
        {
            $(let $vars = $vars.clone();),*
            $closure
        }
    }
}

pub fn load_chips(game: Game, script: impl AsRef<str>) -> Game {
    let game = Rc::new(Mutex::new(game));

    let mut engine = Engine::new();

    // general types
    engine.register_type::<IVec2>();
    engine.register_fn("ivec2", ivec2);
    engine.register_fn("ivec2", |x: i64, y: i64| ivec2(x as i32, y as i32));
    engine.register_type::<Color>();
    engine.register_type::<ChipId>();

    // pin locations
    engine.register_fn("top", |i: i64| Pin::Top(i as usize));
    engine.register_fn("right", |i: i64| Pin::Right(i as usize));
    engine.register_fn("bottom", |i: i64| Pin::Bottom(i as usize));
    engine.register_fn("left", |i: i64| Pin::Left(i as usize));

    // connect overloads
    engine.register_fn(
        "connect",
        shadow! {[game] move |chip_a: ChipId, pin_a: &str, chip_b: ChipId, pin_b: &str| {
            game.lock().unwrap().simulation.connect((chip_a, pin_a), (chip_b, pin_b));
        }},
    );
    engine.register_fn(
        "connect",
        shadow! {[game] move |chip_a: ChipId, pin_a: &str, chip_b: ChipId, pin_b: Pin| {
            game.lock().unwrap().simulation.connect((chip_a, pin_a), (chip_b, pin_b));
        }},
    );
    engine.register_fn(
        "connect",
        shadow! {[game] move |chip_a: ChipId, pin_a: Pin, chip_b: ChipId, pin_b: &str| {
            game.lock().unwrap().simulation.connect((chip_a, pin_a), (chip_b, pin_b));
        }},
    );
    engine.register_fn(
        "connect",
        shadow! {[game] move |chip_a: ChipId, pin_a: Pin, chip_b: ChipId, pin_b: Pin| {
            game.lock().unwrap().simulation.connect((chip_a, pin_a), (chip_b, pin_b));
        }},
    );

    // chip placement
    engine.register_fn(
        "place_clock",
        shadow! {[game] move |interval: i64, at: IVec2| -> ChipId {
            game
                .lock()
                .unwrap()
                .place_chips((Clock::new(interval as usize), at))
                [0]
                .0
        }},
    );

    engine.register_fn(
        "place_counter",
        shadow! { [game] move |at: IVec2| -> ChipId {
            game
                .lock()
                .unwrap()
                .place_chips((Counter8b::default(), at))
                [0]
                .0
        }},
    );

    engine.register_fn(
        "place_high",
        shadow! { [game] move |at: IVec2| {
            game.lock().unwrap().place_chips((TieHigh, at))
            [0].0
        }},
    );

    engine.register_fn(
        "place_cpu",
        shadow! {[game] move |at: IVec2| {
            game.lock().unwrap().place_chips((CPU::default(), at))[0].0
        }},
    );

    engine.run(script.as_ref()).unwrap();

    // explicitly drop engine here so we can move Game back out of the Rc.
    std::mem::drop(engine);

    Rc::try_unwrap(game)
        .expect("Something went wrong!")
        .into_inner()
        .unwrap()
}
