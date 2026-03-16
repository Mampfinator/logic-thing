use nom::{
    IResult, Parser,
    branch::alt,
    bytes::{
        complete::{tag, take_till, take_while1},
        take_while_m_n,
    },
    character::complete::{line_ending, newline, space0, space1},
    combinator::{map, map_res},
    multi::separated_list0,
    sequence::{preceded, terminated},
};

pub trait Parse: Sized {
    fn parse(input: &str) -> IResult<&str, Self>;
}

#[derive(PartialEq, Eq, Debug)]
pub struct Register(u8);

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

#[derive(Debug, PartialEq, Eq)]
pub struct Constant(u16);

impl Parse for Constant {
    fn parse(input: &str) -> IResult<&str, Self> {
        let (input, inner) = alt((parse_hex_u16, parse_dec_u16)).parse(input)?;
        Ok((input, Constant(inner)))
    }
}

#[derive(Debug, PartialEq, Eq)]
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
    #[derive(Debug, PartialEq, Eq)]
    pub enum WritableTarget {
        Register,
        RegisterAddress,
        ConstantAddress
    }
}

define_parse_enum! {
    #[derive(Debug, PartialEq, Eq)]
    pub enum ReadableTarget {
        Register,
        RegisterAddress,
        Constant,
        ConstantAddress
    }
}

#[macro_export]
macro_rules! define_instruction {
    {
        $(#[derive($($derives:ident),+)])?
        $vis:vis struct $type_name:ident as $name:literal { $( $fields:ident: $types:ty),+$(,)? }
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
    };

    (@fields $input:ident $field:ident: $type:ty$(,)?) => {
        let ($input, $field) = Parse::parse($input)?;
        let ($input, _) = space0($input)?;
        let ($input, _) = line_ending($input)?;
    };

    (@fields $input:ident $field:ident: $type:ty, $($rest_fields:ident: $rest_types:ty),+) => {
        let ($input, $field) = Parse::parse($input)?;
        let ($input, _) = space1($input)?;
        define_instruction!(@fields $input $($rest_fields: $rest_types),+);
    };
}

define_instruction! {
    #[derive(Debug, PartialEq, Eq)]
    pub struct Move as "move" {
        from: ReadableTarget,
        to: WritableTarget
    }
}

define_instruction! {

    #[derive(Debug, PartialEq, Eq)]
    pub struct Jump as "jmp" {
        to: Constant,
    }
}

define_parse_enum! {
    #[derive(Debug, PartialEq, Eq)]
    pub enum Instruction {
        Move, Jump
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
            take_till(|c: char| c != '\n'),
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
    //Empty(Commented<Empty>),
}

impl Parse for ProgramLine {
    fn parse(input: &str) -> IResult<&str, Self> {
        alt((
            map(terminated(parse, space0), Self::Instruction),
            //map(terminated(parse, space0), Self::Empty),
        ))
        .parse(input)
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

#[test]
fn test_parsing() {
    let mv = Move::parse("move 10 [0]").unwrap();
    assert_eq!(mv.0, "");

    let (rest_input, result) = ProgramLine::parse("move 10 [0]").unwrap();
    println!("{rest_input}");
    assert_eq!(
        result,
        ProgramLine::Instruction(Commented {
            value: Instruction::Move(Move {
                from: ReadableTarget::Constant(Constant(10)),
                to: WritableTarget::ConstantAddress(Bracketed(Constant(0))),
            }),
            comment: None,
        })
    )
}
