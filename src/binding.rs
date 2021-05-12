use serde::Deserialize;
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
    pub fn to_sql_string(&self) -> anyhow::Result<String> {
        use std::io::Write;
        let mut buf = Vec::new();

        match self {
            Binding::Int(i) => write!(&mut buf, "{}", i)?,
            Binding::Float(float) => write!(&mut buf, "{}", float)?,
            Binding::Bool(b) => write!(&mut buf, "{}", b)?,
            Binding::String(string) => write!(&mut buf, "'{}'", string)?,
            Binding::Json(json) => {
                write!(&mut buf, "'")?;
                serde_json::to_writer(&mut buf, &json)?;
                write!(&mut buf, "'")?;
            }
            Binding::Null => write!(&mut buf, "NULL")?,
        };

        Ok(String::from_utf8(buf)?)
    }

    fn from_json(value: Value) -> anyhow::Result<Self> {
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
}

impl<'de> Deserialize<'de> for Binding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: serde_json::Value = serde_json::Value::deserialize(deserializer)?;
        Binding::from_json(value).map_err(|err| serde::de::Error::custom(err))
    }
}
