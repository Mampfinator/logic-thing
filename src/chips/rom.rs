use macroquad::math::{uvec2, vec2};
use macroquad::ui::{Ui, widgets::Editbox};

use crate::game_objects::chip_inspector::{OpenInspectorPanel, PanelData};
use crate::game_objects::{GameObject, ObjectContextMut};
use crate::{Selection, impl_mgo};

use crate::simulation::{AsInteger, Chip, ChipId, Pin, PinDef, PinLayout, PinsState, Simulation};

const CE: Pin = Pin::Left(0);
const CLK: Pin = Pin::Right(0);

const ADDRESS_PINS: [Pin; 8] = [
    Pin::Left(1),
    Pin::Left(2),
    Pin::Left(3),
    Pin::Left(4),
    Pin::Left(5),
    Pin::Left(6),
    Pin::Left(7),
    Pin::Left(8),
];

const DATA_PINS: [Pin; 8] = [
    Pin::Right(1),
    Pin::Right(2),
    Pin::Right(3),
    Pin::Right(4),
    Pin::Right(5),
    Pin::Right(6),
    Pin::Right(7),
    Pin::Right(8),
];

pub struct ROM {
    content: [u8; 256],
}

impl_mgo!(ROM as ROMObj);

impl From<[u8; 256]> for ROM {
    fn from(value: [u8; 256]) -> Self {
        Self { content: value }
    }
}

impl Chip for ROM {
    fn setup(&self) -> PinLayout {
        PinLayout::new_with(
            uvec2(2, 9),
            DATA_PINS
                .iter()
                .copied()
                .enumerate()
                .map(|(i, pin)| (pin, PinDef::new(format!("D{i}"))))
                .chain(
                    ADDRESS_PINS
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(i, pin)| (pin, PinDef::new(format!("A{i}")))),
                )
                .chain([(CE, PinDef::new("CE")), (CLK, PinDef::new("CLK"))]),
        )
    }

    fn update(&mut self, state: &mut PinsState) {
        if state.read_wire(CE).is_low() {
            return;
        }

        let clock = state.read_wire(CLK);

        if clock.is_falling_edge() {
            let address = state.read_array(&ADDRESS_PINS).into_integer();
            let value = self.content[address as usize];
            state.set_array(&DATA_PINS, value)
        }
    }
}

#[derive(Hash)]
pub struct ROMObj(ChipId);

impl From<ChipId> for ROMObj {
    fn from(value: ChipId) -> Self {
        Self(value)
    }
}

impl GameObject for ROMObj {
    fn start(&mut self, ctx: &mut ObjectContextMut, simulation: &Simulation) {
        self.0.start(ctx, simulation);
    }

    fn on_click(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        if let Selection::Chip(id) = ctx.resource()
            && *id == self.0
        {
            let chip = simulation
                .chips
                .get_mut(self.0)
                .unwrap()
                .downcast_mut::<ROM>()
                .unwrap();

            ctx.insert_resource(OpenInspectorPanel::new(
                self.0,
                ctx.id(),
                PanelData::new(
                    |ui, state| state.ui(ui),
                    |state, _: &mut ROMObj, rom: &mut ROM| {
                        rom.content = state.bytes;
                    },
                    ROMUi::new(self.0, &*chip),
                ),
            ));
        }
        self.0.on_click(ctx, simulation);
    }

    fn update(&mut self, ctx: &mut ObjectContextMut, simulation: &mut Simulation) {
        self.0.update(ctx, simulation);
    }

    fn render(
        &self,
        ctx: &crate::game_objects::ObjectContext,
        simulation: &Simulation,
        objects: &crate::game_objects::GameObjects,
    ) {
        self.0.render(ctx, simulation, objects);
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
}

struct ROMUi {
    chip_id: ChipId,
    bytes: [u8; 256],
    text: [[String; 16]; 16],
}

impl ROMUi {
    pub fn ui(&mut self, ui: &mut Ui) {
        let id = self.chip_id.0;
        for (row_id, row) in self.text.iter_mut().enumerate() {
            for (column_id, string) in row.iter_mut().enumerate() {
                let last_len = string.len();
                let edited = Editbox::new((id + row_id * 16 + column_id) as u64, vec2(20., 20.))
                    .filter(&|char| char.is_ascii_hexdigit() && last_len <= 1)
                    .position(vec2(column_id as f32 * 22., row_id as f32 * 22.))
                    .ui(ui, string);

                if edited && let Ok(byte) = u8::from_str_radix(string, 16) {
                    self.bytes[row_id * 16 + column_id] = byte;
                }
            }
        }
    }
}

impl ROMUi {
    pub fn new(chip_id: ChipId, rom: &ROM) -> Self {
        Self {
            chip_id,
            bytes: rom.content.clone(),
            text: (0..16)
                .map(|x| {
                    (0..16)
                        .map(|y| {
                            let index = x * 16 + y;
                            format!("{:02x}", rom.content[index])
                        })
                        .collect::<Vec<_>>()
                        .try_into()
                        .unwrap()
                })
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        }
    }
}

impl ROM {
    pub fn set(&mut self, offset: usize, byte: u8) {
        self.content[offset] = byte;
    }
}
