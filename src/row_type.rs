use anyhow::anyhow;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{
    database::HasValueRef,
    postgres::{PgTypeInfo, PgTypeKind, PgValueRef},
    Decode, Postgres, Type, ValueRef,
};
use sqlx::{Column, ColumnIndex, Row, TypeInfo};
use std::{borrow::Borrow, collections::BTreeMap, error::Error};

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
pub enum RowType {
    Bool(Option<bool>),
    BoolArray(Option<Vec<bool>>),
    Bytea(Option<Vec<u8>>),
    ByteaArray(Option<Vec<Vec<u8>>>),
    Char(Option<i8>),
    CharArray(Option<Vec<i8>>),
    Name(Option<String>),
    NameArray(Option<Vec<String>>),
    Int8(Option<i64>),
    Int8Array(Option<Vec<i64>>),
    Int2(Option<i16>),
    Int2Array(Option<Vec<i16>>),
    Int4(Option<i32>),
    Int4Array(Option<Vec<i32>>),
    Text(Option<String>),
    TextArray(Option<Vec<String>>),
    Json(Option<Value>),
    JsonArray(Option<Vec<Value>>),
    // Unknown,
    // Point,
    // Lseg,
    // Path,
    // Box,
    // Polygon,
    // Line,
    // Cidr,
    Float4(Option<f32>),
    Float4Array(Option<Vec<f32>>),
    Float8(Option<f64>),
    Float8Array(Option<Vec<f64>>),
    // Unknown,
    // Circle,
    // Macaddr8,
    // Macaddr,
    // Inet,
    Bpchar(Option<String>),
    BpcharArray(Option<Vec<String>>),
    Varchar(Option<String>),
    VarcharArray(Option<Vec<String>>),
    Date(Option<NaiveDate>),
    DateArray(Option<Vec<NaiveDate>>),
    Time(Option<NaiveTime>),
    TimeArray(Option<Vec<NaiveTime>>),
    Timestamp(Option<NaiveDateTime>),
    TimestampArray(Option<Vec<NaiveDateTime>>),
    Timestamptz(Option<DateTime<Utc>>),
    TimestamptzArray(Option<Vec<DateTime<Utc>>>),
    // Interval,
    // Timetz,
    // Bit,
    // Varbit,
    // Numeric,
    // Record,
    Uuid(Option<uuid::Uuid>),
    UuidArray(Option<Vec<uuid::Uuid>>),
    Jsonb(Option<Value>),
    JsonbArray(Option<Vec<Value>>),
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
    let map = row.columns()
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
    let type_info = value_ref.type_info();
    let row_type: RowType = match type_info.name() {
        "BOOL" => RowType::Bool(try_get(value_ref)?),
        "BOOL[]" => RowType::BoolArray(try_get(value_ref)?),
        "BYTEA" => RowType::Bytea(try_get(value_ref)?),
        "BYTEA[]" => RowType::ByteaArray(try_get(value_ref)?),
        "CHAR" => RowType::Char(try_get(value_ref)?),
        "CHAR[]" => RowType::CharArray(try_get(value_ref)?),
        "DATE" => RowType::Date(try_get(value_ref)?),
        "DATE[]" => RowType::DateArray(try_get(value_ref)?),
        "FLOAT4" => RowType::Float4(try_get(value_ref)?),
        "FLOAT4[]" => RowType::Float4Array(try_get(value_ref)?),
        "FLOAT8" => RowType::Float8(try_get(value_ref)?),
        "FLOAT8[]" => RowType::Float8Array(try_get(value_ref)?),
        "INT2" => RowType::Int2(try_get(value_ref)?),
        "INT2[]" => RowType::Int2Array(try_get(value_ref)?),
        "INT4" => RowType::Int4(try_get(value_ref)?),
        "INT4[]" => RowType::Int4Array(try_get(value_ref)?),
        "INT8" => RowType::Int8(try_get(value_ref)?),
        "INT8[]" => RowType::Int8Array(try_get(value_ref)?),
        "JSON" => RowType::Json(try_get(value_ref)?),
        "JSON[]" => RowType::JsonArray(try_get(value_ref)?),
        "JSONB" => RowType::Jsonb(try_get(value_ref)?),
        "JSONB[]" => RowType::JsonbArray(try_get(value_ref)?),
        "NAME" => RowType::Name(try_get(value_ref)?),
        "NAME[]" => RowType::NameArray(try_get(value_ref)?),
        "TEXT" => RowType::Text(try_get(value_ref)?),
        "TEXT[]" => RowType::TextArray(try_get(value_ref)?),
        "TIME" => RowType::Time(try_get(value_ref)?),
        "TIME[]" => RowType::TimeArray(try_get(value_ref)?),
        "TIMESTAMP" => RowType::Timestamp(try_get(value_ref)?),
        "TIMESTAMP[]" => RowType::TimestampArray(try_get(value_ref)?),
        "TIMESTAMPTZ" => RowType::Timestamptz(try_get(value_ref)?),
        "TIMESTAMPTZ[]" => RowType::TimestamptzArray(try_get(value_ref)?),
        "UUID" => RowType::Uuid(try_get(value_ref)?),
        "UUID[]" => RowType::UuidArray(try_get(value_ref)?),
        "VARCHAR" => RowType::Varchar(try_get(value_ref)?),
        "VARCHAR[]" => RowType::VarcharArray(try_get(value_ref)?),
        "\"CHAR\"" => RowType::Char(try_get(value_ref)?),
        "\"CHAR\"[]" => RowType::Char(try_get(value_ref)?),
        // "BIT" => todo!(),
        // "BOX" => todo!(),
        // "CIDR" => todo!(),
        // "CIRCLE" => todo!(),
        // "DATERANGE" => RowType::DateRange,
        // "INET" => todo!(),
        // "INT4RANGE" => todo!(),
        // "INT8RANGE" => todo!(),
        // "INTERVAL" => todo!(),
        // "JSONPATH" => todo!(),
        // "LINE" => todo!(),
        // "LSEG" => todo!(),
        // "MACADDR" => todo!(),
        // "MACADDR8" => todo!(),
        // "MONEY" => todo!(),
        // "NUMERIC" => todo!(),
        // "NUMRANGE" => todo!(),
        // "PATH" => todo!(),
        // "POINT" => todo!(),
        // "POLYGON" => todo!(),
        // "RECORD" => todo!(),
        // "TIMETZ" => todo!(),
        // "TSRANGE" => todo!(),
        // "TSTZRANGE" => todo!(),
        // "VARBIT" => todo!(),
        // "OID" => todo!(),
        // "VOID" => todo!(),
        // "UNKNOWN" => todo!(),
        _ => Err(anyhow!(
            "type parsing for {} is not implemented yet",
            type_info.name()
        ))?,
    };

    Ok(row_type)
}

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
