//! Header-addressed access to one CSV record and the [`Field`] / [`TableRow`] parsing traits.

use std::collections::HashMap;

use crate::error::{Result, TableError};

/// The header line of a table file: column name → position.
#[derive(Debug, Clone)]
pub struct Header {
    table: String,
    columns: HashMap<String, usize>,
}

impl Header {
    /// Builds the column index from the header record of `table`.
    pub fn new(table: &str, columns: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            table: table.to_owned(),
            columns: columns
                .into_iter()
                .enumerate()
                .map(|(i, c)| (c.into(), i))
                .collect(),
        }
    }

    /// Name of the table this header belongs to.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Position of `column`, if the file has it.
    pub fn position(&self, column: &str) -> Option<usize> {
        self.columns.get(column).copied()
    }

    /// Whether the file has `column`.
    pub fn has(&self, column: &str) -> bool {
        self.columns.contains_key(column)
    }
}

/// One record of a table file, addressed by column name.
#[derive(Debug, Clone, Copy)]
pub struct Row<'a> {
    header: &'a Header,
    record: &'a csv::StringRecord,
    line: u64,
}

impl<'a> Row<'a> {
    /// Wraps `record`, which was read at `line` (1-based) of the file described by `header`.
    pub fn new(header: &'a Header, record: &'a csv::StringRecord, line: u64) -> Self {
        Self {
            header,
            record,
            line,
        }
    }

    /// Name of the table the row comes from.
    pub fn table(&self) -> &str {
        self.header.table()
    }

    /// Line of the file the row was read from (1-based, as reported by the CSV reader).
    pub fn line(&self) -> u64 {
        self.line
    }

    /// The raw text of `column`.
    pub fn get(&self, column: &str) -> Result<&'a str> {
        let position = self
            .header
            .position(column)
            .ok_or_else(|| TableError::MissingColumn {
                table: self.table().to_owned(),
                column: column.to_owned(),
            })?;
        // A record that is shorter than the header cannot happen with a strict CSV reader; treat
        // it as an empty cell rather than a panic if it ever does.
        Ok(self.record.get(position).unwrap_or(""))
    }

    /// The value of `column` parsed as `T`.
    pub fn field<T: Field>(&self, column: &str) -> Result<T> {
        T::parse_field(self, column)
    }

    fn parse_error(&self, column: &str, value: &str, ty: &'static str) -> TableError {
        TableError::Parse {
            table: self.table().to_owned(),
            line: self.line,
            column: column.to_owned(),
            value: value.to_owned(),
            ty,
        }
    }
}

/// A type that can be read from one column (or, for arrays, a run of numbered columns).
pub trait Field: Sized {
    /// Parses the field named `column` of `row`.
    fn parse_field(row: &Row<'_>, column: &str) -> Result<Self>;
}

impl Field for String {
    fn parse_field(row: &Row<'_>, column: &str) -> Result<Self> {
        Ok(row.get(column)?.to_owned())
    }
}

impl Field for bool {
    fn parse_field(row: &Row<'_>, column: &str) -> Result<Self> {
        let value = row.get(column)?.trim();
        match value {
            "" | "0" | "false" => Ok(false),
            "1" | "true" => Ok(true),
            _ => Err(row.parse_error(column, value, "bool")),
        }
    }
}

/// Parses an integer cell. Empty cells read as `0`. A negative value in an unsigned column is
/// reinterpreted as the two's-complement bit pattern of the column's width, because the dumps
/// write masks that way (`-1` = all bits, `-2147483648` = bit 31).
fn parse_int(
    row: &Row<'_>,
    column: &str,
    bits: u32,
    signed: bool,
    ty: &'static str,
) -> Result<i128> {
    let value = row.get(column)?.trim();
    if value.is_empty() {
        return Ok(0);
    }
    let parsed: i128 = value
        .parse()
        .map_err(|_| row.parse_error(column, value, ty))?;
    let (min, max) = if signed {
        (-(1i128 << (bits - 1)), (1i128 << (bits - 1)) - 1)
    } else {
        (0, (1i128 << bits) - 1)
    };
    if parsed >= min && parsed <= max {
        return Ok(parsed);
    }
    if !signed && parsed < 0 && parsed >= -(1i128 << (bits - 1)) {
        return Ok(parsed + (1i128 << bits));
    }
    Err(row.parse_error(column, value, ty))
}

macro_rules! int_field {
    ($($ty:ty => ($bits:expr, $signed:expr)),* $(,)?) => {
        $(
            impl Field for $ty {
                fn parse_field(row: &Row<'_>, column: &str) -> Result<Self> {
                    parse_int(row, column, $bits, $signed, stringify!($ty)).map(|v| v as $ty)
                }
            }
        )*
    };
}

int_field! {
    u8 => (8, false), u16 => (16, false), u32 => (32, false), u64 => (64, false),
    i8 => (8, true), i16 => (16, true), i32 => (32, true), i64 => (64, true),
}

macro_rules! float_field {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Field for $ty {
                fn parse_field(row: &Row<'_>, column: &str) -> Result<Self> {
                    let value = row.get(column)?.trim();
                    if value.is_empty() {
                        return Ok(0.0);
                    }
                    value
                        .parse()
                        .map_err(|_| row.parse_error(column, value, stringify!($ty)))
                }
            }
        )*
    };
}

float_field!(f32, f64);

/// Numbered columns `<prefix>0 .. <prefix>N-1` (`Attributes_0..16`, `EffectMiscValue_0/1`, ...)
/// read as one array; `column` is the prefix including the underscore.
impl<T: Field, const N: usize> Field for [T; N] {
    fn parse_field(row: &Row<'_>, column: &str) -> Result<Self> {
        let mut values = Vec::with_capacity(N);
        for i in 0..N {
            values.push(T::parse_field(row, &format!("{column}{i}"))?);
        }
        // The vector has exactly N elements, so the conversion cannot fail.
        Ok(values
            .try_into()
            .unwrap_or_else(|_| unreachable!("array field of {N} columns")))
    }
}

/// A typed row of one table.
pub trait TableRow: Sized {
    /// The table name, i.e. the file name without `.<build>.csv`.
    const TABLE: &'static str;

    /// Parses one record.
    fn from_row(row: &Row<'_>) -> Result<Self>;
}

/// Declares a [`TableRow`] struct whose fields map to named columns.
///
/// ```ignore
/// table_row! {
///     /// One `SpellDuration` row.
///     SpellDurationRow, "SpellDuration" {
///         id: u32 = "ID",
///         duration: i32 = "Duration",
///         preset_spells: [u32; 8] = "PresetSpellID_",   // PresetSpellID_0 .. PresetSpellID_7
///     }
/// }
/// ```
#[macro_export]
macro_rules! table_row {
    (
        $(#[$meta:meta])*
        $name:ident, $table:literal {
            $( $(#[$field_meta:meta])* $field:ident : $ty:ty = $column:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            $( $(#[$field_meta])* pub $field: $ty, )*
        }

        impl $crate::row::TableRow for $name {
            const TABLE: &'static str = $table;

            fn from_row(row: &$crate::row::Row<'_>) -> $crate::error::Result<Self> {
                Ok(Self {
                    $( $field: <$ty as $crate::row::Field>::parse_field(row, $column)?, )*
                })
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_of<'a>(header: &'a Header, record: &'a csv::StringRecord) -> Row<'a> {
        Row::new(header, record, 2)
    }

    fn header() -> Header {
        Header::new(
            "T",
            ["ID", "Mask", "Name", "Arr_0", "Arr_1", "F", "Empty", "Flag"],
        )
    }

    fn record(cells: &[&str]) -> csv::StringRecord {
        csv::StringRecord::from(cells.to_vec())
    }

    #[test]
    fn scalar_fields_parse_by_column_name() {
        let header = header();
        let record = record(&[
            "12294",
            "-2147483648",
            "Mortal Strike",
            "3",
            "-4",
            "0.5",
            "",
            "1",
        ]);
        let row = row_of(&header, &record);
        assert_eq!(row.field::<u32>("ID").unwrap(), 12294);
        assert_eq!(row.field::<String>("Name").unwrap(), "Mortal Strike");
        assert_eq!(row.field::<f64>("F").unwrap(), 0.5);
        assert!(row.field::<bool>("Flag").unwrap());
        assert_eq!(row.table(), "T");
        assert_eq!(row.line(), 2);
    }

    #[test]
    fn negative_values_in_unsigned_columns_are_bit_patterns() {
        let header = header();
        let record = record(&["-1", "-2147483648", "", "0", "0", "0", "", "0"]);
        let row = row_of(&header, &record);
        assert_eq!(row.field::<u32>("ID").unwrap(), u32::MAX);
        assert_eq!(row.field::<u32>("Mask").unwrap(), 1 << 31);
        assert_eq!(row.field::<i32>("Mask").unwrap(), i32::MIN);
        assert_eq!(row.field::<u8>("ID").unwrap(), 255);
    }

    #[test]
    fn empty_numeric_cells_read_as_zero() {
        let header = header();
        let record = record(&["1", "0", "", "0", "0", "", "", ""]);
        let row = row_of(&header, &record);
        assert_eq!(row.field::<u32>("Empty").unwrap(), 0);
        assert_eq!(row.field::<i64>("Empty").unwrap(), 0);
        assert_eq!(row.field::<f32>("F").unwrap(), 0.0);
        assert!(!row.field::<bool>("Flag").unwrap());
        assert_eq!(row.field::<String>("Name").unwrap(), "");
    }

    #[test]
    fn out_of_range_and_garbage_values_are_errors() {
        let header = header();
        let record = record(&["4294967296", "abc", "x", "0", "0", "1.5.2", "", "2"]);
        let row = row_of(&header, &record);
        let err = row.field::<u32>("ID").unwrap_err();
        assert!(
            matches!(err, TableError::Parse { ref column, line: 2, ty: "u32", .. } if column == "ID")
        );
        assert!(row.field::<i32>("Mask").is_err());
        assert!(row.field::<f64>("F").is_err());
        assert!(row.field::<bool>("Flag").is_err());
        assert!(row.field::<i8>("ID").is_err());
    }

    #[test]
    fn arrays_read_numbered_columns() {
        let header = header();
        let record = record(&["1", "0", "", "3", "-4", "0", "", "0"]);
        let row = row_of(&header, &record);
        assert_eq!(row.field::<[i32; 2]>("Arr_").unwrap(), [3, -4]);
        assert!(row.field::<[i32; 3]>("Arr_").is_err());
    }

    #[test]
    fn missing_column_is_reported_with_table_name() {
        let header = header();
        let record = record(&["1", "0", "", "3", "-4", "0", "", "0"]);
        let row = row_of(&header, &record);
        let err = row.field::<u32>("Nope").unwrap_err();
        assert_eq!(err.to_string(), "T: column Nope is missing from the header");
    }

    table_row! {
        /// Test row.
        TestRow, "T" {
            id: u32 = "ID",
            name: String = "Name",
            arr: [i32; 2] = "Arr_",
        }
    }

    #[test]
    fn table_row_macro_builds_typed_rows() {
        let header = header();
        let record = record(&["7", "0", "Seven", "1", "2", "0", "", "0"]);
        let row = row_of(&header, &record);
        assert_eq!(TestRow::TABLE, "T");
        assert_eq!(
            TestRow::from_row(&row).unwrap(),
            TestRow {
                id: 7,
                name: "Seven".into(),
                arr: [1, 2]
            }
        );
    }
}
