use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{postgres::PgValueRef, Decode, Postgres, Type, ValueRef};
use sqlx::{Column, Row, TypeInfo};
use std::collections::BTreeMap;

// bool	BOOL
// i8	  CHAR
// i16	SMALLINT, SMALLSERIAL, INT2
// i32	INT, SERIAL, INT4
// i64	BIGINT, BIGSERIAL, INT8
// f32	REAL, FLOAT4
// f64	DOUBLE PRECISION, FLOAT8
// &[u8] BYTEA
// PgInterval	INTERVAL
// &str, String	VARCHAR, CHAR(N), TEXT, NAME
// PgRange<T>	INT8RANGE, INT4RANGE, TSRANGE, TSTZTRANGE, DATERANGE, NUMRANGE

// bool	BOOL
// i8	  CHAR
// i16	SMALLINT
// i16  SMALLSERIAL
// i16  INT2
// i32	INT
// i32  SERIAL
// i32  INT4
// i64	BIGINT
// i64  BIGSERIAL
// i64  INT8
// f32	REAL
// f32  FLOAT4
// f64	DOUBLE PRECISION
// f64  FLOAT8
// Vec<u8> BYTEA
// PgInterval	INTERVAL
// String	VARCHAR
// String CHAR(N)
// String  TEXT
// String  NAME

// PgRange<T>	INT8RANGE, INT4RANGE, TSRANGE, TSTZTRANGE, DATERANGE, NUMRANGE

#[derive(Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Category<T> {
    Value(Option<T>),
    Array(Option<Vec<Option<T>>>),
}

#[derive(Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum RowType {
    Bool(Category<bool>),
    Bytea(Category<Vec<u8>>),
    Char(Category<i8>),
    Name(Category<String>),
    Int8(Category<i64>),
    Int2(Category<i16>),
    Int4(Category<i32>),
    Text(Category<String>),
    Json(Category<Value>),
    // Unknown,
    // Point,
    // Lseg,
    // Path,
    // Box,
    // Polygon,
    // Line,
    // Cidr,
    Float4(Category<f32>),
    Float8(Category<f64>),
    // Unknown,
    // Circle,
    // Macaddr8,
    // Macaddr,
    // Inet,
    // Bpchar(Category<String>),
    Varchar(Category<String>),
    Date(Category<NaiveDate>),
    Time(Category<NaiveTime>),
    Timestamp(Category<NaiveDateTime>),
    Timestamptz(Category<DateTime<Utc>>),
    // Interval,
    // Timetz,
    // Bit,
    // Varbit,
    // Numeric,
    // Record,
    Uuid(Category<uuid::Uuid>),
    Jsonb(Category<Value>),
    // Int4Range(),
    // NumRange,
    // TsRange,
    // TstzRange,
    // DateRange,
    // Int8Range,
    // Jsonpath,
    // Money,
}

fn try_get<'r, T>(value: PgValueRef<'r>) -> anyhow::Result<T>
where
    T: Decode<'r, Postgres> + Type<Postgres>,
{
    if !value.is_null() {
        let ty = value.type_info();

        if !ty.is_null() && !T::compatible(&ty) {
            return Err(anyhow!(
                "type {} is not compatible with type {}",
                ty.name(),
                T::type_info().name(),
            ));
        }
    }

    T::decode(value.clone()).map_err(|_| {
        return anyhow!("failed to decode for type {}", T::type_info().name(),);
    })
}

pub fn convert_row(row: sqlx::postgres::PgRow) -> anyhow::Result<BTreeMap<String, RowType>> {
    let map = row
        .columns()
        .iter()
        .map(|col| -> anyhow::Result<_> {
            let name = col.name();
            let value_ref = row.try_get_raw(col.ordinal()).map_err(|err| {
                anyhow!("could not get column {} due to {}", name, err.to_string())
            })?;

            Ok((name.to_string(), convert_value(value_ref)?))
        })
        .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
    Ok(map)
}

fn convert_value(value_ref: PgValueRef) -> anyhow::Result<RowType> {
    use Category::{Array, Value};
    let type_info = value_ref.type_info();
    let row_type: RowType = match type_info.name() {
        "BOOL" => RowType::Bool(Value(try_get(value_ref)?)),
        "BOOL[]" => RowType::Bool(Array(try_get(value_ref)?)),
        "BYTEA" => RowType::Bytea(Value(try_get(value_ref)?)),
        "BYTEA[]" => RowType::Bytea(Array(try_get(value_ref)?)),
        "CHAR" => RowType::Char(Value(try_get(value_ref)?)),
        "CHAR[]" => RowType::Char(Array(try_get(value_ref)?)),
        "DATE" => RowType::Date(Value(try_get(value_ref)?)),
        "DATE[]" => RowType::Date(Array(try_get(value_ref)?)),
        "FLOAT4" => RowType::Float4(Value(try_get(value_ref)?)),
        "FLOAT4[]" => RowType::Float4(Array(try_get(value_ref)?)),
        "FLOAT8" => RowType::Float8(Value(try_get(value_ref)?)),
        "FLOAT8[]" => RowType::Float8(Array(try_get(value_ref)?)),
        "INT2" => RowType::Int2(Value(try_get(value_ref)?)),
        "INT2[]" => RowType::Int2(Array(try_get(value_ref)?)),
        "INT4" => RowType::Int4(Value(try_get(value_ref)?)),
        "INT4[]" => RowType::Int4(Array(try_get(value_ref)?)),
        "INT8" => RowType::Int8(Value(try_get(value_ref)?)),
        "INT8[]" => RowType::Int8(Array(try_get(value_ref)?)),
        "JSON" => RowType::Json(Value(try_get(value_ref)?)),
        "JSON[]" => RowType::Json(Array(try_get(value_ref)?)),
        "JSONB" => RowType::Jsonb(Value(try_get(value_ref)?)),
        "JSONB[]" => RowType::Jsonb(Array(try_get(value_ref)?)),
        "NAME" => RowType::Name(Value(try_get(value_ref)?)),
        "NAME[]" => RowType::Name(Array(try_get(value_ref)?)),
        "TEXT" => RowType::Text(Value(try_get(value_ref)?)),
        "TEXT[]" => RowType::Text(Array(try_get(value_ref)?)),
        "TIME" => RowType::Time(Value(try_get(value_ref)?)),
        "TIME[]" => RowType::Time(Array(try_get(value_ref)?)),
        "TIMESTAMP" => RowType::Timestamp(Value(try_get(value_ref)?)),
        "TIMESTAMP[]" => RowType::Timestamp(Array(try_get(value_ref)?)),
        "TIMESTAMPTZ" => RowType::Timestamptz(Value(try_get(value_ref)?)),
        "TIMESTAMPTZ[]" => RowType::Timestamptz(Array(try_get(value_ref)?)),
        "UUID" => RowType::Uuid(Value(try_get(value_ref)?)),
        "UUID[]" => RowType::Uuid(Array(try_get(value_ref)?)),
        "VARCHAR" => RowType::Varchar(Value(try_get(value_ref)?)),
        "VARCHAR[]" => RowType::Varchar(Array(try_get(value_ref)?)),
        "\"CHAR\"" => RowType::Char(Value(try_get(value_ref)?)),
        "\"CHAR\"[]" => RowType::Char(Value(try_get(value_ref)?)),
        // TODO: 
        // "BIT" => {},
        // "BOX" => {},
        // "CIDR" => {},
        // "CIRCLE" => {},
        // "DATERANGE" => {},
        // "INET" => {},
        // "INT4RANGE" => {},
        // "INT8RANGE" => {},
        // "INTERVAL" => {},
        // "JSONPATH" => {},
        // "LINE" => {},
        // "LSEG" => {},
        // "MACADDR" => {},
        // "MACADDR8" => {},
        // "MONEY" => {},
        // "NUMERIC" => {},
        // "NUMRANGE" => {},
        // "PATH" => {},
        // "POINT" => {},
        // "POLYGON" => {},
        // "RECORD" => {},
        // "TIMETZ" => {},
        // "TSRANGE" => {},
        // "TSTZRANGE" => {},
        // "VARBIT" => {},
        // "OID" => {},
        // "VOID" => {},
        // "UNKNOWN" => {},
        _ => Err(anyhow!(
            "type parsing for {} is not implemented yet",
            type_info.name()
        ))?,
    };

    Ok(row_type)
}

#[allow(dead_code)]
const ALL_TYPES: [&'static str; 92] = [
    "BIT",
    "BIT[]",
    "BOOL",
    "BOOL[]",
    "BOX",
    "BOX[]",
    "BYTEA",
    "BYTEA[]",
    "CHAR",
    "CHAR[]",
    "CIDR",
    "CIDR[]",
    "CIRCLE",
    "CIRCLE[]",
    "DATE",
    "DATERANGE",
    "DATERANGE[]",
    "DATE[]",
    "FLOAT4",
    "FLOAT4[]",
    "FLOAT8",
    "FLOAT8[]",
    "INET",
    "INET[]",
    "INT2",
    "INT2[]",
    "INT4",
    "INT4RANGE",
    "INT4RANGE[]",
    "INT4[]",
    "INT8",
    "INT8RANGE",
    "INT8RANGE[]",
    "INT8[]",
    "INTERVAL",
    "INTERVAL[]",
    "JSON",
    "JSONB",
    "JSONB[]",
    "JSONPATH",
    "JSONPATH[]",
    "JSON[]",
    "LINE",
    "LINE[]",
    "LSEG",
    "LSEG[]",
    "MACADDR",
    "MACADDR8",
    "MACADDR8[]",
    "MACADDR[]",
    "MONEY",
    "MONEY[]",
    "NAME",
    "NAME[]",
    "NUMERIC",
    "NUMERIC[]",
    "NUMRANGE",
    "NUMRANGE[]",
    "OID",
    "OID[]",
    "PATH",
    "PATH[]",
    "POINT",
    "POINT[]",
    "POLYGON",
    "POLYGON[]",
    "RECORD",
    "RECORD[]",
    "TEXT",
    "TEXT[]",
    "TIME",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "TIMESTAMPTZ[]",
    "TIMESTAMP[]",
    "TIMETZ",
    "TIMETZ[]",
    "TIME[]",
    "TSRANGE",
    "TSRANGE[]",
    "TSTZRANGE",
    "TSTZRANGE[]",
    "UNKNOWN",
    "UUID",
    "UUID[]",
    "VARBIT",
    "VARBIT[]",
    "VARCHAR",
    "VARCHAR[]",
    "VOID",
    "\"CHAR\"",
    "\"CHAR\"[]",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_test() {
        let naive_date = NaiveDate::from_ymd(2015, 3, 14);
        assert_eq!(
            serde_json::to_string(&naive_date).ok(),
            Some(r#""2015-03-14""#.to_string())
        );

        let naive_date =
            DateTime::<Utc>::from_utc(NaiveDateTime::from_timestamp(1_700_000_000, 0), Utc);
        assert_eq!(
            serde_json::to_string(&naive_date).ok(),
            Some(r#""2023-11-14T22:13:20Z""#.to_string())
        );
    }
}
