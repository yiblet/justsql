use anyhow::anyhow;
use serde_json::Value;

pub enum Binding {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Json(Value),
    Null,
}

impl Binding {
    pub fn from_json(value: Value) -> anyhow::Result<Self> {
        let val = match value {
            Value::Null => Binding::Null,
            Value::Bool(val) => Binding::Bool(val),
            Value::String(string) => Binding::String(string),
            Value::Number(number) => {
                if number.is_i64() {
                    Binding::Int(number.as_i64().unwrap())
                } else if number.is_u64() {
                    Err(anyhow!(
                        "number {} is out of bounds for postgres",
                        number.as_u64().unwrap()
                    ))?
                } else if number.is_f64() {
                    Binding::Float(number.as_f64().unwrap())
                } else {
                    Err(anyhow!("unexpected number type",))?
                }
            }
            _ => Binding::Json(value),
        };

        Ok(val)
    }

    fn bind<'a, 'b: 'a>(
        &'b self,
        query: sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments>,
    ) -> sqlx::query::Query<'a, sqlx::Postgres, sqlx::postgres::PgArguments> {
        match self {
            Self::Int(val) => query.bind(val),
            Self::Bool(val) => query.bind(val),
            Self::Float(val) => query.bind(val),
            Self::String(val) => query.bind(val),
            Self::Null => {
                // TODO check if this doesn't cause some kind of type error
                // if the null type is the wrong null
                let null: Option<String> = None;
                query.bind(null)
            }
            Self::Json(val) => query.bind(val),
        }
    }
}
