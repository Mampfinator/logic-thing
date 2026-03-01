use macroquad::math::uvec2;

use crate::simulation::{Chip, Pin, PinDef, PinLayout, PinsState};

trait AsBits<const SIZE: usize> {
    fn as_bits(&self) -> [bool; SIZE];
}

impl AsBits<8> for u8 {
    fn as_bits(&self) -> [bool; 8] {
        (0..8)
            .map(|shift| (self >> shift) & 1 > 0)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }
}

impl AsBits<16> for u16 {
    fn as_bits(&self) -> [bool; 16] {
        (0..16)
            .map(|shift| (self >> shift) & 1 > 0)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }
}

/// Simple 8 bit CPU (with 8 bit addresses for now because I'm lazy.)
#[derive(Default)]
pub struct CPU {
    program_counter: u16,
    state: DecodeState,
}

pub const DATA_PINS: [Pin; 8] = [
    Pin::Left(4),
    Pin::Left(5),
    Pin::Left(6),
    Pin::Left(7),
    Pin::Left(8),
    Pin::Left(9),
    Pin::Left(10),
    Pin::Left(11),
];

pub const ADDRESS_PINS: [Pin; 16] = [
    Pin::Right(0),
    Pin::Right(1),
    Pin::Right(2),
    Pin::Right(3),
    Pin::Right(4),
    Pin::Right(5),
    Pin::Right(6),
    Pin::Right(7),
    Pin::Right(8),
    Pin::Right(9),
    Pin::Right(10),
    Pin::Right(11),
    Pin::Right(12),
    Pin::Right(13),
    Pin::Left(13),
    Pin::Left(12),
];

/// Whether the CPU is enabled.
const CE: Pin = Pin::Left(0);
/// Interrupt request.
const IRQ: Pin = Pin::Left(1);
/// Clock input.
const CLK: Pin = Pin::Left(2);
/// Whether the CPU is writing to the data data lines, or reading from them.
const RW: Pin = Pin::Left(3);

impl Chip for CPU {
    fn setup(&self) -> PinLayout {
        PinLayout::new_with(
            uvec2(4, 14),
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
                .chain(
                    [
                        (CE, PinDef::new("CE")),
                        (IRQ, PinDef::new("IRQ")),
                        (CLK, PinDef::new("CLK")),
                        (RW, PinDef::new("RW")),
                    ]
                    .into_iter(),
                ),
        )
    }
    fn update(&mut self, state: &mut PinsState) {
        if state.read_wire(CE).is_low() {
            return;
        }

        if state.read_wire(IRQ).is_rising_edge() {
            unimplemented!("Interrupts, yippee :)");
        }

        let clock = state.read_wire(CLK);
        if clock.is_rising_edge() {
            for (index, bit) in self.program_counter.as_bits().into_iter().enumerate() {
                state.set(ADDRESS_PINS[index], bit);
            }
            self.program_counter = self.program_counter.wrapping_add(1);
            return;
        } else if clock.is_falling_edge() {
            let Some(instruction) = self
                .state
                .try_decode(state)
                .unwrap_or(Some(Instruction::Nop))
            else {
                return;
            };

            instruction.run(state, self);
        }
    }
}

#[derive(Debug)]
enum DecodeError {
    Other(String),
}

#[derive(Default)]
struct DecodeState {
    bytes: Vec<u8>,
}

impl DecodeState {
    pub fn reset(&mut self) {
        self.bytes.clear();
    }

    pub fn try_decode(
        &mut self,
        state: &mut PinsState,
    ) -> Result<Option<Instruction>, DecodeError> {
        let byte = DATA_PINS
            .iter()
            .copied()
            .enumerate()
            .map(|(index, pin)| (state.read_wire(pin).is_high() as u8) << index)
            .fold(0_u8, |acc, bit| acc | bit);

        self.bytes.push(byte);

        let result = self.try_parse_instruction();

        if matches!(result, Ok(Some(_))) {
            self.bytes.clear();
        }

        result
    }

    pub fn try_parse_instruction(&self) -> Result<Option<Instruction>, DecodeError> {
        if self.bytes.len() > 4 {
            Err(DecodeError::Other("invalid instruction".into()))
        } else {
            Ok(Instruction::try_parse(&self.bytes))
        }
    }
}

enum Instruction {
    Nop,
    Move(u8, u8),
    Jump(u16),
}

impl Instruction {
    pub fn try_parse(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [0x1, target_high, target_low] => {
                Some(Self::Jump(u16::from_le_bytes([*target_high, *target_low])))
            }
            [0x2, from, to] => Some(Self::Move(*from, *to)),
            _ => None,
        }
    }

    pub fn run(self, state: &mut PinsState, cpu: &mut CPU) {
        match self {
            Self::Nop => {}
            Self::Move(from, to) => {
                println!("Move {from:x} {to:x}");
            }
            Self::Jump(to) => {
                cpu.program_counter = to;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cpu::AsBits;

    #[test]
    fn test_as_bits() {
        let test: u8 = 0b10100101;
        assert_eq!(
            test.as_bits(),
            [true, false, true, false, false, true, false, true]
        );
    }
}
