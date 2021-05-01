use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
#[serde(untagged)]
pub enum Binding {
    Int(i64),
    Float(f64),
    Bool(bool),
    String(String),
    Json(Value),
    Null,
}

pub fn bindings_from_json(
    payload: BTreeMap<String, Value>,
) -> anyhow::Result<BTreeMap<String, Binding>> {
    let bindings: BTreeMap<String, Binding> = payload
        .into_iter()
        .map(|(val, res)| Ok((val, Binding::from_json(res)?)))
        .collect::<anyhow::Result<BTreeMap<String, Binding>>>()?;
    Ok(bindings)
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
}
