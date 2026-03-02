use macroquad::math::uvec2;

use crate::impl_mgo;

use crate::simulation::{AsInteger, Chip, Pin, PinDef, PinLayout, PinsState};

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

impl_mgo!(ROM);

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
            let address = state.read_array(&ADDRESS_PINS).as_integer();
            let value = self.content[address as usize];
            state.set_array(&DATA_PINS, value)
        }
    }
}
