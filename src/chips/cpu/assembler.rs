use nom::{
    IResult, Parser,
    branch::alt,
    bytes::{
        complete::{tag, take, take_till, take_while1},
        take_while_m_n,
    },
    character::complete::{newline, space0, space1},
    combinator::{map, map_opt, map_res},
    multi::separated_list0,
    sequence::{preceded, terminated},
};

pub trait AsBytes: Sized {
    fn as_bytes(&self) -> impl IntoIterator<Item = u8>;
}

pub trait TryFromBytes: Sized {
    fn try_from_bytes(bytes: &[u8]) -> IResult<&[u8], Self>;
}

pub trait Parse: Sized {
    fn parse(input: &str) -> IResult<&str, Self>;
}

#[derive(PartialEq, Eq, Debug, Clone, Copy)]
pub struct Register(u8);

impl AsBytes for Register {
    fn as_bytes(&self) -> impl IntoIterator<Item = u8> {
        std::iter::once(self.0)
    }
}

impl TryFromBytes for Register {
    fn try_from_bytes(bytes: &[u8]) -> IResult<&[u8], Self> {
        map(take_next, Self).parse(bytes)
    }
}

impl Parse for Register {
    fn parse(input: &str) -> IResult<&str, Register> {
        let (input, _) = tag("r")(input)?;
        let (input, num) = map_res(take_while1(|c: char| c.is_ascii_digit()), |str: &str| {
            str.parse::<u8>()
        })
        .parse(input)?;

        Ok((input, Register(num)))
    }
}

fn parse_dec_u16(input: &str) -> IResult<&str, u16> {
    map_res(take_while1(|c: char| c.is_ascii_digit()), str::parse).parse(input)
}

fn parse_hex_u16(input: &str) -> IResult<&str, u16> {
    let (input, _) = tag("#")(input)?;
    map_res(
        take_while_m_n(4, 4, |c: char| c.is_digit(16)),
        |input: &str| u16::from_str_radix(input, 16),
    )
    .parse(input)
}

fn parse_bin_u16(input: &str) -> IResult<&str, u16> {
    let (input, _) = tag("0b")(input)?;
    map_res(
        take_while_m_n(16, 16, |c: char| c == '0' || c == '1'),
        |input: &str| u16::from_str_radix(input, 2),
    )
    .parse(input)
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Constant(u16);

impl AsBytes for Constant {
    fn as_bytes(&self) -> impl IntoIterator<Item = u8> {
        self.0.to_le_bytes()
    }
}

impl TryFromBytes for Constant {
    fn try_from_bytes(bytes: &[u8]) -> IResult<&[u8], Self> {
        map(take_const::<2>, |bytes| Self(u16::from_le_bytes(bytes))).parse(bytes)
    }
}

impl Parse for Constant {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (input, inner) = alt((parse_hex_u16, parse_dec_u16)).parse(input)?;
        Ok((input, Constant(inner)))
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Bracketed<T: Parse>(pub T);
impl<T: Parse> Parse for Bracketed<T> {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (input, _) = tag("[")(input)?;
        let (input, inner) = T::parse(input)?;
        let (input, _) = tag("]")(input)?;

        Ok((input, Self(inner)))
    }
}

fn parse<T: Parse>(input: &str) -> IResult<&str, T> {
    T::parse(input)
}

#[macro_export]
macro_rules! define_parse_enum {
    {
        $(#[derive($($derives:ident),+)])?
        $vis:vis enum $type_name:ident { $($variant:ident),+ }
    } => {
        $(#[derive($($derives),*)])?
        $vis enum $type_name {
            $($variant($variant)),+
        }

        #[automatically_derived]
        impl Parse for $type_name {
            fn parse(input: &str) -> IResult<&str, Self> {
                alt((
                    $(map(parse, Self::$variant)),*,
                )).parse(input)
            }
        }
    }
}

pub type RegisterAddress = Bracketed<Register>;
pub type ConstantAddress = Bracketed<Constant>;

define_parse_enum! {
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum WritableTarget {
        Register,
        RegisterAddress,
        ConstantAddress
    }
}

enum AddressingMode {
    Register,
    RegisterAddr,
    Const,
    ConstAddr,
}

impl AsBytes for AddressingMode {
    fn as_bytes(&self) -> impl IntoIterator<Item = u8> {
        std::iter::once(match self {
            Self::Register => MODE_REGISTER,
            Self::RegisterAddr => MODE_REGISTER_ADDR,
            Self::Const => MODE_CONST,
            Self::ConstAddr => MODE_CONST_ADDR,
        })
    }
}

fn take_const<const N: usize>(bytes: &[u8]) -> IResult<&[u8], [u8; N]> {
    map_res(take(N), |bytes: &[u8]| bytes.try_into()).parse(bytes)
}

fn take_next(bytes: &[u8]) -> IResult<&[u8], u8> {
    map(take(1usize), |taken: &[u8]| taken[0]).parse(bytes)
}

impl TryFromBytes for AddressingMode {
    fn try_from_bytes(bytes: &[u8]) -> IResult<&[u8], Self> {
        map_opt(take_next, AddressingMode::try_from_byte).parse(bytes)
    }
}

impl AddressingMode {
    pub fn try_from_byte(byte: u8) -> Option<Self> {
        match byte {
            MODE_REGISTER => Some(Self::Register),
            MODE_REGISTER_ADDR => Some(Self::RegisterAddr),
            MODE_CONST => Some(Self::Const),
            MODE_CONST_ADDR => Some(Self::ConstAddr),
            _ => None,
        }
    }
}

pub const MODE_REGISTER: u8 = 0x10;
pub const MODE_REGISTER_ADDR: u8 = 0x11;
pub const MODE_CONST: u8 = 0x00;
pub const MODE_CONST_ADDR: u8 = 0x01;

// Targets prefix their inner value with an addressing mode prefix.
impl AsBytes for WritableTarget {
    fn as_bytes(&self) -> impl IntoIterator<Item = u8> {
        let vec: Vec<u8> = match self {
            Self::Register(reg) => std::iter::once(MODE_REGISTER)
                .chain(reg.as_bytes())
                .collect(),
            Self::RegisterAddress(reg) => std::iter::once(MODE_REGISTER_ADDR)
                .chain(reg.0.as_bytes())
                .collect(),
            Self::ConstantAddress(con) => std::iter::once(MODE_CONST_ADDR)
                .chain(con.0.as_bytes())
                .collect(),
        };

        vec
    }
}

impl TryFromBytes for WritableTarget {
    fn try_from_bytes(in_bytes: &[u8]) -> IResult<&[u8], Self> {
        let (bytes, mode) = AddressingMode::try_from_bytes(in_bytes)?;
        match mode {
            AddressingMode::Register => map(from_bytes, Self::Register).parse(bytes),
            AddressingMode::ConstAddr => {
                map(from_bytes, |con| Self::ConstantAddress(Bracketed(con))).parse(bytes)
            }
            AddressingMode::RegisterAddr => {
                map(from_bytes, |reg| Self::RegisterAddress(Bracketed(reg))).parse(bytes)
            }
            _ => Err(nom::Err::Error(nom::error::Error::new(
                in_bytes,
                nom::error::ErrorKind::Alpha,
            ))),
        }
    }
}

define_parse_enum! {
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    pub enum ReadableTarget {
        Register,
        RegisterAddress,
        Constant,
        ConstantAddress
    }
}

fn from_bytes<T: TryFromBytes>(bytes: &[u8]) -> IResult<&[u8], T> {
    T::try_from_bytes(bytes)
}

// Targets prefix their inner value with an addressing mode prefix.
impl AsBytes for ReadableTarget {
    fn as_bytes(&self) -> impl IntoIterator<Item = u8> {
        let vec: Vec<u8> = match self {
            Self::Register(reg) => std::iter::once(MODE_REGISTER)
                .chain(reg.as_bytes())
                .collect(),
            Self::RegisterAddress(reg) => std::iter::once(MODE_REGISTER_ADDR)
                .chain(reg.0.as_bytes())
                .collect(),
            Self::Constant(con) => std::iter::once(MODE_CONST).chain(con.as_bytes()).collect(),
            Self::ConstantAddress(con) => std::iter::once(MODE_CONST_ADDR)
                .chain(con.0.as_bytes())
                .collect(),
        };

        vec
    }
}

impl TryFromBytes for ReadableTarget {
    fn try_from_bytes(bytes: &[u8]) -> IResult<&[u8], Self> {
        let (bytes, mode) = AddressingMode::try_from_bytes(bytes)?;
        match mode {
            AddressingMode::Const => map(from_bytes, Self::Constant).parse(bytes),
            AddressingMode::Register => map(from_bytes, Self::Register).parse(bytes),
            AddressingMode::ConstAddr => {
                map(from_bytes, |con| Self::ConstantAddress(Bracketed(con))).parse(bytes)
            }
            AddressingMode::RegisterAddr => {
                map(from_bytes, |reg| Self::RegisterAddress(Bracketed(reg))).parse(bytes)
            }
        }
    }
}

#[macro_export]
macro_rules! define_instruction {
    {
        $(#[derive($($derives:ident),+)])?
        $vis:vis struct $type_name:ident as $name:literal | $op:literal { $( $fields:ident: $types:ty),+$(,)? }
    } => {
        $(#[derive($($derives),*)])?
        $vis struct $type_name {
            $($fields: $types),*
        }

        #[automatically_derived]
        impl Parse for $type_name {
            fn parse(input: &str) -> IResult<&str, Self> {
                let (input, _) = tag($name).parse(input)?;
                let (input, _) = space1(input)?;

                define_instruction!(@fields input $($fields: $types),+);

                Ok((input, Self {
                    $($fields),*
                }))

            }
        }

        #[automatically_derived]
        impl AsBytes for $type_name {
            fn as_bytes(&self) -> impl IntoIterator<Item = u8> {
                std::iter::once($op)
                $(.chain(self. $fields.as_bytes()))*
            }
        }

        #[automatically_derived]
        impl TryFromBytes for $type_name {
            fn try_from_bytes(bytes: &[u8]) -> IResult<&[u8], Self> {
                let (bytes, _) = map_res(take_next, |byte| if byte == $op { Ok(byte) } else { Err(()) }).parse(bytes)?;
                $(let (bytes, $fields) = TryFromBytes::try_from_bytes(bytes)?;)*

                Ok((bytes, Self {
                    $($fields),*
                }))
            }
        }
    };

    // unpack fields; spaces after the last field are optional.
    (@fields $input:ident $field:ident: $type:ty$(,)?) => {
        let ($input, $field) = Parse::parse($input)?;
        let ($input, _) = space0($input)?;
    };

    (@fields $input:ident $field:ident: $type:ty, $($rest_fields:ident: $rest_types:ty),+) => {
        let ($input, $field) = Parse::parse($input)?;
        let ($input, _) = space1($input)?;
        define_instruction!(@fields $input $($rest_fields: $rest_types),+);
    };
}

define_instruction! {
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct Move as "mov" | 0x01 {
        from: ReadableTarget,
        to: WritableTarget
    }
}

define_instruction! {

    #[derive(Debug, PartialEq, Eq, Clone)]
    pub struct Jump as "jmp" | 0x02 {
        to: Constant,
    }
}

define_parse_enum! {
    #[derive(Debug, PartialEq, Eq, Clone)]
    pub enum Instruction {
        Move, Jump
    }
}

impl AsBytes for Instruction {
    fn as_bytes(&self) -> impl IntoIterator<Item = u8> {
        let vec: Vec<u8> = match self {
            Self::Move(inner) => inner.as_bytes().into_iter().collect(),
            Self::Jump(inner) => inner.as_bytes().into_iter().collect(),
        };
        vec
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Empty;
impl Parse for Empty {
    fn parse(input: &str) -> IResult<&str, Self> {
        if input.is_empty() {
            Err(nom::Err::Incomplete(nom::Needed::Unknown))
        } else {
            Ok((input, Empty))
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Commented<T: Parse> {
    pub value: T,
    comment: Option<String>,
}

impl<T: Parse> Parse for Commented<T> {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (input, value) = T::parse(input)?;
        let (input, comment) = preceded(
            (space0::<_, (&str, _)>, tag("#")),
            take_till(|c: char| c == '\n'),
        )
        .parse(input)
        .map(|(input, comment)| (input, Some(comment.to_owned())))
        .unwrap_or((input, None));

        Ok((input, Self { value, comment }))
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ProgramLine {
    Instruction(Commented<Instruction>),
    Label(String),
}

impl Parse for ProgramLine {
    fn parse(input: &str) -> IResult<&str, Self> {
        alt((map(terminated(parse, space0), Self::Instruction),)).parse(input)
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Program {
    lines: Vec<ProgramLine>,
}

impl Parse for Program {
    fn parse(input: &str) -> IResult<&str, Self> {
        map(
            separated_list0(newline, preceded(space0, ProgramLine::parse)),
            |lines| Self { lines },
        )
        .parse(input)
    }
}

impl Program {
    pub fn compile(self) -> Vec<u8> {
        let mut out = Vec::new();
        for instruction in self.lines.into_iter().filter_map(|line| {
            if let ProgramLine::Instruction(instruction) = line {
                Some(instruction.value)
            } else {
                None
            }
        }) {
            out.extend(instruction.as_bytes());
        }

        out
    }
}

pub fn compile_to_bytes(input: &str) -> Option<Vec<u8>> {
    let (_, program) = Program::parse(input).ok()?;
    Some(program.compile())
}

pub fn fill_to<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    if N < bytes.len() {
        None
    } else {
        let padded_bytes = bytes
            .iter()
            .copied()
            .chain(std::iter::repeat(0))
            .take(N)
            .collect::<Vec<_>>();

        Some(padded_bytes.try_into().unwrap())
    }
}

#[test]
fn test_parsing_and_to_bytes() {
    let (rest_input, result) = ProgramLine::parse("mov 10 [0] # Hello, world!").unwrap();
    assert_eq!(rest_input, "");
    assert_eq!(
        result,
        ProgramLine::Instruction(Commented {
            value: Instruction::Move(Move {
                from: ReadableTarget::Constant(Constant(10)),
                to: WritableTarget::ConstantAddress(Bracketed(Constant(0))),
            }),
            comment: Some(" Hello, world!".into()),
        })
    );

    let ProgramLine::Instruction(instruction) = result else {
        panic!();
    };

    let parsed_bytes = instruction.value.as_bytes().into_iter().collect::<Vec<_>>();

    #[rustfmt::skip]
    let bytes = [
        // mov
        0x01, 
        // 10
        MODE_CONST, 10, 0,
        // [0] 
        MODE_CONST_ADDR, 0, 0
    ];

    assert_eq!(parsed_bytes, bytes);

    let Instruction::Move(mv) = instruction.value else {
        panic!();
    };

    let (_, byte_mv) = Move::try_from_bytes(&bytes).unwrap();

    assert_eq!(byte_mv, mv);
}
