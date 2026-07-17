
use macroquad::math::{UVec2, uvec2};
use rhai::{
    AST, Dynamic, Engine, EvalAltResult, Scope,
};

use crate::{
    game_objects::{GameObject, GameObjects, ObjectContext, ObjectContextMut},
    impl_mgo,
    simulation::{
        Chip, ChipId, ComputedPinsState, MutationBuffer, Pin, PinDef, PinLayout, PinsState,
        Simulation,
    },
};

// TODO: add some sort of state to programmable chips. This can just be a hashmap on our end in the typestore that can be accessed during update.

pub struct ProgrammableChip {
    engine: Engine,
    scope: Scope<'static>,
    ast: AST,
}

impl ProgrammableChip {
    pub fn from_code(code: &str) -> Option<Self> {
        let mut engine = Engine::new();
        engine
            .register_type::<UVec2>()
            .register_fn("uvec2", uvec2)
            .register_fn("uvec2", |x: i64, y: i64| uvec2(x as u32, y as u32))
            .register_type::<Pin>()
            .register_fn("top", Pin::Top)
            .register_fn("top", |i: i64| Pin::Top(i as usize))
            .register_fn("right", Pin::Right)
            .register_fn("right", |i: i64| Pin::Right(i as usize))
            .register_fn("bottom", Pin::Bottom)
            .register_fn("bottom", |i: i64| Pin::Bottom(i as usize))
            .register_fn("left", Pin::Left)
            .register_fn("left", |i: i64| Pin::Left(i as usize))
            .register_type::<PinDef>()
            .register_fn("pin", |label: &str| PinDef::new(label))
            .register_fn("pin_with_state", |label: &str, state: bool| {
                PinDef::new_with_state(label, state)
            })
            .register_type::<PinLayout>()
            .register_fn("make_layout", |size: UVec2, items: rhai::Array| {
                PinLayout::new_with(
                    size,
                    items.into_iter().map(|value| {
                        let mut arr = value.cast::<rhai::Array>();
                        debug_assert_eq!(arr.len(), 2);
                        let def = arr.pop().unwrap().cast::<PinDef>();
                        let pin = arr.pop().unwrap().cast::<Pin>();
                        (pin, def)
                    }),
                )
            })
            .register_type::<ComputedPinsState>()
            .register_fn("read_wire", ComputedPinsState::read_wire)
            .register_fn("read_output", ComputedPinsState::read_output)
            .register_fn("set", ComputedPinsState::set)
            .register_fn("toggle", ComputedPinsState::toggle)
            .register_type::<MutationBuffer>()
            .register_fn("mutations", MutationBuffer::default)
            .register_fn("mutate", MutationBuffer::mutate);

        let ast = engine.compile(code).unwrap();

        let mut scope = Scope::new();

        engine.run_ast_with_scope(&mut scope, &ast).unwrap();

        Some(Self { engine, scope, ast })
    }
}

fn map_optional_function_result<T>(
    value: Result<T, Box<EvalAltResult>>,
    name: &str,
) -> Option<Result<T, Box<EvalAltResult>>> {
    match value {
        Ok(v) => Some(Ok(v)),
        Err(e) => match *e {
            EvalAltResult::ErrorFunctionNotFound(func, pos) => {
                if func == name {
                    None
                } else {
                    Some(Err(Box::new(EvalAltResult::ErrorFunctionNotFound(
                        func, pos,
                    ))))
                }
            }
            e => Some(Err(Box::new(e))),
        },
    }
}

impl Chip for ProgrammableChip {
    fn setup(&mut self) -> PinLayout {
        self.engine
            .call_fn(&mut self.scope, &self.ast, "setup", ())
            .unwrap()
    }

    fn update(&mut self, state: &mut PinsState) {
        match map_optional_function_result(
            self.engine.call_fn::<Dynamic>(
                &mut self.scope,
                &self.ast,
                "update",
                (state.as_computed(),),
            ),
            "update",
        ) {
            None => {}
            Some(Ok(inner)) => {
                if let Some(mutations) = inner.try_cast::<MutationBuffer>() {
                    state.replace_mutations(mutations);
                }
            }
            Some(Err(e)) => {
                eprintln!("Error updating ProgrammableChip: {}", e)
            }
        }
    }
}

macro_rules! make_shared_rhai_handle {
    ($handle_type:ident, $origin_type:ty) => {
        #[derive(Debug, Eq, PartialEq, Clone, Hash)]
        pub struct $handle_type(usize, u32);

        impl $handle_type {
            pub fn new(value: &$origin_type) -> Self {
                let handle = unsafe { std::mem::transmute(value) };
                let id = macroquad::prelude::rand::rand();
                Self(handle, id)
            }
        }

        impl AsRef<$origin_type> for $handle_type {
            fn as_ref(&self) -> &$origin_type {
                unsafe { &*std::ptr::with_exposed_provenance::<$origin_type>(self.0) }
            }
        }
    };
}

make_shared_rhai_handle!(SharedSimulationHandle, Simulation);
make_shared_rhai_handle!(SharedObjectsHandle, GameObjects);
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct SharedContextHandle(usize, u32);
impl SharedContextHandle {
    pub fn new(value: &ObjectContext) -> Self {
        let handle = unsafe { std::mem::transmute(value) };
        let id = macroquad::prelude::rand::rand();
        Self(handle, id)
    }
}

impl<'a> AsRef<ObjectContext<'a>> for SharedContextHandle {
    fn as_ref(&self) -> &ObjectContext<'a> {
        unsafe { &*std::ptr::with_exposed_provenance::<ObjectContext<'_>>(self.0) }
    }
}

#[derive(Hash, Clone, Copy, Debug)]
pub struct ProgrammableChipObj(ChipId);
impl From<ChipId> for ProgrammableChipObj {
    fn from(value: ChipId) -> Self {
        Self(value)
    }
}

impl GameObject for ProgrammableChipObj {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        self.0.start(ctx, simulation)
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_click(ctx, simulation)
    }

    fn on_click_released(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_click_released(ctx, simulation);
    }

    fn on_mouse_enter(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_mouse_enter(ctx, simulation);
    }

    fn on_mouse_exit(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.on_mouse_exit(ctx, simulation);
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.update(ctx, simulation);
    }

    fn render(&self, ctx: &ObjectContext, simulation: &Simulation, objects: &GameObjects) {
        let instance = simulation.chips.get(self.0).unwrap();
        let chip = instance.downcast_ref::<ProgrammableChip>().unwrap();

        let mut scope = chip.scope.clone();
        if let Some(inner) = map_optional_function_result(
            chip.engine
                .call_fn::<()>(&mut scope, &chip.ast, "render", ()),
            "render",
        ) {
            inner.unwrap();
        } else {
            self.0.render(ctx, simulation, objects)
        }
    }
}

impl_mgo!(ProgrammableChip as ProgrammableChipObj);
