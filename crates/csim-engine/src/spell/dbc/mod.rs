//! The client table vocabulary: the enums and bit flags that `SpellEffect.Effect`,
//! `SpellEffect.EffectAura`, `SpellMisc.Attributes_0`, `SpellAuraOptions.ProcTypeMask_0`, … store.
//!
//! The numeric values are the retail client's (TrinityCore `SharedDefines.h` /
//! `SpellDefines.h` names), which is what the Forever build `1.60.1.70009` dumps use; see
//! `data/SPELL_INSTRUCTIONS.md` §1.7 for which of them the player spells actually carry.
//!
//! Every enum here follows the same contract so that a new dump never fails to load:
//!
//! - it is a closed list of the values the engine knows plus `Unknown(id)` for everything else;
//! - `id()` / `from_id()` round-trip, `name()` / `from_name()` map the known values to their
//!   `SCREAMING_SNAKE_CASE` retail name;
//! - serde accepts **either the name or the number** and serializes the name (the number for an
//!   unknown value), so exported data files stay readable while raw table values still load.
//!
//! Bit-flag types ([`SpellAttr0`] to [`SpellAttr3`], [`ProcFlags`], [`SpellSchoolMask`]) serialize
//! as the raw number and deserialize from a number, a single flag name, or a list of names /
//! numbers, so hand-written overrides can say `proc_type_mask: [DEAL_MELEE_SWING, DEAL_MELEE_ABILITY]`.

mod aura;
mod discard;
mod effect;
mod flags;
mod misc;

pub use aura::AuraType;
pub use discard::{DISCARDED_AURA_IDS, DISCARDED_EFFECT_IDS};
pub use effect::SpellEffectName;
pub use flags::{
    ProcFlags, SpellAttr0, SpellAttr1, SpellAttr2, SpellAttr3, SpellAttr8, SpellSchoolMask,
};
pub use misc::{
    AuraState, DefenseType, ImplicitTarget, Mechanic, PowerType, ShapeshiftForm, SpellModOp,
};

/// Declares a table enum: known variants with their retail id and name, plus `Unknown(id)`.
macro_rules! dbc_enum {
    (
        $(#[$meta:meta])*
        $name:ident : $repr:ty {
            $( $(#[$vmeta:meta])* $variant:ident = $id:literal => $str:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )*
            /// A value the engine has no name for (kept verbatim so it can be reported).
            Unknown($repr),
        }

        impl $name {
            /// Every known value, in declaration order.
            pub const KNOWN: &'static [Self] = &[$(Self::$variant),*];

            /// The retail numeric value.
            pub const fn id(self) -> $repr {
                match self {
                    $( Self::$variant => $id, )*
                    Self::Unknown(id) => id,
                }
            }

            /// The value with this numeric id; `Unknown(id)` if the engine has no name for it.
            pub const fn from_id(id: $repr) -> Self {
                match id {
                    $( $id => Self::$variant, )*
                    other => Self::Unknown(other),
                }
            }

            /// The retail name, `None` for unknown values.
            pub const fn name(self) -> Option<&'static str> {
                match self {
                    $( Self::$variant => Some($str), )*
                    Self::Unknown(_) => None,
                }
            }

            /// The value with this retail name.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $( $str => Some(Self::$variant), )*
                    _ => None,
                }
            }

            /// Whether the engine has a name for this value.
            pub const fn is_known(self) -> bool {
                !matches!(self, Self::Unknown(_))
            }
        }

        impl Default for $name {
            /// The value with id 0 (`NONE` for most enums).
            fn default() -> Self {
                Self::from_id(0)
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                match self.name() {
                    Some(name) => f.write_str(name),
                    None => write!(f, "UNKNOWN_{}", self.id()),
                }
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::spell::dbc::ParseError;

            /// Parses a retail name, a number, or `UNKNOWN_<n>`.
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let s = s.trim();
                if let Some(value) = Self::from_name(s) {
                    return Ok(value);
                }
                let digits = s.strip_prefix("UNKNOWN_").unwrap_or(s);
                digits
                    .parse::<$repr>()
                    .map(Self::from_id)
                    .map_err(|_| $crate::spell::dbc::ParseError {
                        ty: stringify!($name),
                        value: s.to_owned(),
                    })
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                match self.name() {
                    Some(name) => serializer.serialize_str(name),
                    None => ::serde::Serialize::serialize(&self.id(), serializer),
                }
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Visitor;

                impl<'de> ::serde::de::Visitor<'de> for Visitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        write!(f, "a {} name or number", stringify!($name))
                    }

                    fn visit_u64<E: ::serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                        <$repr>::try_from(v)
                            .map($name::from_id)
                            .map_err(|_| E::custom(format!("{} out of range for {}", v, stringify!($name))))
                    }

                    fn visit_i64<E: ::serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                        <$repr>::try_from(v)
                            .map($name::from_id)
                            .map_err(|_| E::custom(format!("{} out of range for {}", v, stringify!($name))))
                    }

                    fn visit_str<E: ::serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                        v.parse().map_err(E::custom)
                    }
                }

                deserializer.deserialize_any(Visitor)
            }
        }
    };
}

/// Declares a bit-flag type over `u32` with named bits.
macro_rules! dbc_flags {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $(#[$fmeta:meta])* $flag:ident = $bit:literal => $str:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
        pub struct $name(u32);

        impl $name {
            $( $(#[$fmeta])* pub const $flag: Self = Self($bit); )*

            /// Every named bit with its name, in declaration order.
            pub const NAMED: &'static [(Self, &'static str)] = &[$((Self::$flag, $str)),*];

            /// No bits.
            pub const fn empty() -> Self {
                Self(0)
            }

            /// The flags with exactly these bits (unknown bits are kept).
            pub const fn from_bits(bits: u32) -> Self {
                Self(bits)
            }

            /// The raw bits.
            pub const fn bits(self) -> u32 {
                self.0
            }

            /// Whether no bit is set.
            pub const fn is_empty(self) -> bool {
                self.0 == 0
            }

            /// Whether every bit of `other` is set.
            pub const fn contains(self, other: Self) -> bool {
                self.0 & other.0 == other.0
            }

            /// Whether any bit of `other` is set.
            pub const fn intersects(self, other: Self) -> bool {
                self.0 & other.0 != 0
            }

            /// The union of both flag sets.
            pub const fn union(self, other: Self) -> Self {
                Self(self.0 | other.0)
            }

            /// The flag with this name.
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $( $str => Some(Self::$flag), )*
                    _ => None,
                }
            }

            /// Names of the set bits the engine knows, in bit order.
            pub fn names(self) -> Vec<&'static str> {
                Self::NAMED
                    .iter()
                    .filter(|(flag, _)| self.contains(*flag))
                    .map(|(_, name)| *name)
                    .collect()
            }

            /// The set bits the engine has no name for.
            pub fn unknown_bits(self) -> u32 {
                let known = Self::NAMED.iter().fold(0, |acc, (flag, _)| acc | flag.0);
                self.0 & !known
            }
        }

        impl ::std::ops::BitOr for $name {
            type Output = Self;
            fn bitor(self, rhs: Self) -> Self {
                Self(self.0 | rhs.0)
            }
        }

        impl ::std::ops::BitOrAssign for $name {
            fn bitor_assign(&mut self, rhs: Self) {
                self.0 |= rhs.0;
            }
        }

        impl ::std::ops::BitAnd for $name {
            type Output = Self;
            fn bitand(self, rhs: Self) -> Self {
                Self(self.0 & rhs.0)
            }
        }

        impl ::std::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self)
            }
        }

        impl ::std::fmt::Display for $name {
            /// `A | B | 0x100` (unknown bits as one hex remainder); `0` when empty.
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                if self.0 == 0 {
                    return f.write_str("0");
                }
                let mut first = true;
                for name in self.names() {
                    if !first {
                        f.write_str(" | ")?;
                    }
                    first = false;
                    f.write_str(name)?;
                }
                let unknown = self.unknown_bits();
                if unknown != 0 {
                    if !first {
                        f.write_str(" | ")?;
                    }
                    write!(f, "{unknown:#x}")?;
                }
                Ok(())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $crate::spell::dbc::ParseError;

            /// Parses `A | B`, a decimal or `0x` hex number, or a mix of both.
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let mut flags = Self::empty();
                for part in s.split('|') {
                    let part = part.trim();
                    if part.is_empty() {
                        continue;
                    }
                    flags |= $crate::spell::dbc::parse_flag_part(part, |name| {
                        Self::from_name(name).map(Self::bits)
                    })
                    .map(Self::from_bits)
                        .ok_or_else(|| $crate::spell::dbc::ParseError {
                            ty: stringify!($name),
                            value: part.to_owned(),
                        })?;
                }
                Ok(flags)
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u32(self.0)
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                struct Visitor;

                impl<'de> ::serde::de::Visitor<'de> for Visitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        write!(f, "a {} number, name or list of names", stringify!($name))
                    }

                    fn visit_u64<E: ::serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
                        u32::try_from(v)
                            .map($name::from_bits)
                            .map_err(|_| E::custom(format!("{v} does not fit in 32 bits")))
                    }

                    fn visit_i64<E: ::serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
                        // The dumps write bit 31 as a negative number.
                        i32::try_from(v)
                            .map(|v| $name::from_bits(v as u32))
                            .map_err(|_| E::custom(format!("{v} does not fit in 32 bits")))
                    }

                    fn visit_str<E: ::serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
                        v.parse().map_err(E::custom)
                    }

                    fn visit_seq<A: ::serde::de::SeqAccess<'de>>(
                        self,
                        mut seq: A,
                    ) -> Result<Self::Value, A::Error> {
                        let mut flags = $name::empty();
                        while let Some(part) = seq.next_element::<$name>()? {
                            flags |= part;
                        }
                        Ok(flags)
                    }
                }

                deserializer.deserialize_any(Visitor)
            }
        }
    };
}

pub(crate) use dbc_enum;
pub(crate) use dbc_flags;

/// A name or number that is not a value of the table enum it was parsed as.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{value:?} is not a {ty} name or number")]
pub struct ParseError {
    /// The enum or flag type.
    pub ty: &'static str,
    /// The offending text.
    pub value: String,
}

/// Parses one `|`-separated part of a flag string: a flag name, a decimal or a `0x` hex number.
pub(crate) fn parse_flag_part(part: &str, from_name: impl Fn(&str) -> Option<u32>) -> Option<u32> {
    if let Some(bits) = from_name(part) {
        return Some(bits);
    }
    if let Some(hex) = part.strip_prefix("0x").or_else(|| part.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok();
    }
    if let Ok(value) = part.parse::<u32>() {
        return Some(value);
    }
    // The dumps write bit 31 as a negative number.
    part.parse::<i32>().ok().map(|v| v as u32)
}

#[cfg(test)]
mod tests {
    dbc_enum! {
        /// Test enum.
        Sample: u32 {
            Zero = 0 => "ZERO",
            Two = 2 => "TWO",
        }
    }

    dbc_flags! {
        /// Test flags.
        SampleFlags {
            A = 0x1 => "A",
            B = 0x2 => "B",
            HIGH = 0x8000_0000 => "HIGH",
        }
    }

    #[test]
    fn enum_ids_names_and_unknowns_round_trip() {
        assert_eq!(Sample::KNOWN, [Sample::Zero, Sample::Two]);
        assert_eq!(Sample::from_id(2), Sample::Two);
        assert_eq!(Sample::from_id(7), Sample::Unknown(7));
        assert_eq!(Sample::Unknown(7).id(), 7);
        assert_eq!(Sample::Two.name(), Some("TWO"));
        assert_eq!(Sample::Unknown(7).name(), None);
        assert_eq!(Sample::from_name("TWO"), Some(Sample::Two));
        assert_eq!(Sample::from_name("two"), None);
        assert!(Sample::Two.is_known());
        assert!(!Sample::Unknown(7).is_known());
        assert_eq!(Sample::Two.to_string(), "TWO");
        assert_eq!(Sample::Unknown(7).to_string(), "UNKNOWN_7");
        assert_eq!("TWO".parse::<Sample>().unwrap(), Sample::Two);
        assert_eq!(" 2 ".parse::<Sample>().unwrap(), Sample::Two);
        assert_eq!("UNKNOWN_7".parse::<Sample>().unwrap(), Sample::Unknown(7));
        let err = "nope".parse::<Sample>().unwrap_err();
        assert_eq!(err.to_string(), "\"nope\" is not a Sample name or number");
    }

    #[test]
    fn enum_serde_accepts_names_and_numbers_and_writes_names() {
        assert_eq!(serde_yaml::from_str::<Sample>("TWO").unwrap(), Sample::Two);
        assert_eq!(serde_yaml::from_str::<Sample>("2").unwrap(), Sample::Two);
        assert_eq!(
            serde_yaml::from_str::<Sample>("\"2\"").unwrap(),
            Sample::Two
        );
        assert_eq!(
            serde_yaml::from_str::<Sample>("9").unwrap(),
            Sample::Unknown(9)
        );
        assert!(serde_yaml::from_str::<Sample>("-1").is_err());
        assert!(serde_yaml::from_str::<Sample>("nope").is_err());
        assert_eq!(serde_yaml::to_string(&Sample::Two).unwrap().trim(), "TWO");
        assert_eq!(
            serde_yaml::to_string(&Sample::Unknown(9)).unwrap().trim(),
            "9"
        );
        let list: Vec<Sample> = serde_yaml::from_str("[ZERO, 2, 5]").unwrap();
        assert_eq!(list, [Sample::Zero, Sample::Two, Sample::Unknown(5)]);
    }

    #[test]
    fn flags_combine_and_report_unknown_bits() {
        let flags = SampleFlags::A | SampleFlags::HIGH | SampleFlags::from_bits(0x10);
        assert_eq!(flags.bits(), 0x8000_0011);
        assert!(flags.contains(SampleFlags::A));
        assert!(!flags.contains(SampleFlags::B));
        assert!(flags.intersects(SampleFlags::B | SampleFlags::HIGH));
        assert_eq!(flags.names(), ["A", "HIGH"]);
        assert_eq!(flags.unknown_bits(), 0x10);
        assert_eq!(flags.to_string(), "A | HIGH | 0x10");
        assert_eq!(SampleFlags::empty().to_string(), "0");
        assert!(SampleFlags::empty().is_empty());
        assert_eq!(format!("{:?}", SampleFlags::B), "SampleFlags(B)");
        assert_eq!((flags & SampleFlags::A), SampleFlags::A);
        assert_eq!(SampleFlags::A.union(SampleFlags::B).bits(), 3);
        let mut acc = SampleFlags::empty();
        acc |= SampleFlags::B;
        assert_eq!(acc, SampleFlags::B);
    }

    #[test]
    fn flags_parse_names_numbers_and_mixes() {
        assert_eq!("A | B".parse::<SampleFlags>().unwrap().bits(), 3);
        assert_eq!("0x10".parse::<SampleFlags>().unwrap().bits(), 0x10);
        assert_eq!("18".parse::<SampleFlags>().unwrap().bits(), 18);
        assert_eq!(
            "-2147483648".parse::<SampleFlags>().unwrap(),
            SampleFlags::HIGH
        );
        assert_eq!("A|0x10".parse::<SampleFlags>().unwrap().bits(), 0x11);
        assert_eq!("".parse::<SampleFlags>().unwrap(), SampleFlags::empty());
        assert!("A | C".parse::<SampleFlags>().is_err());
        assert!("4294967296".parse::<SampleFlags>().is_err());
    }

    #[test]
    fn flags_serde_accepts_numbers_names_and_lists_and_writes_numbers() {
        assert_eq!(serde_yaml::from_str::<SampleFlags>("3").unwrap().bits(), 3);
        assert_eq!(
            serde_yaml::from_str::<SampleFlags>("0x80000000").unwrap(),
            SampleFlags::HIGH
        );
        assert_eq!(
            serde_yaml::from_str::<SampleFlags>("-2147483648").unwrap(),
            SampleFlags::HIGH
        );
        assert_eq!(
            serde_yaml::from_str::<SampleFlags>("A").unwrap(),
            SampleFlags::A
        );
        assert_eq!(
            serde_yaml::from_str::<SampleFlags>("[A, B, 0x10]")
                .unwrap()
                .bits(),
            0x13
        );
        assert_eq!(
            serde_yaml::from_str::<SampleFlags>("[]").unwrap(),
            SampleFlags::empty()
        );
        assert!(serde_yaml::from_str::<SampleFlags>("[A, nope]").is_err());
        assert_eq!(
            serde_yaml::to_string(&SampleFlags::A.union(SampleFlags::B))
                .unwrap()
                .trim(),
            "3"
        );
        assert_eq!(
            serde_yaml::to_string(&SampleFlags::HIGH).unwrap().trim(),
            "2147483648"
        );
    }
}
