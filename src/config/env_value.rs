use std::borrow::Cow;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum EnvValue<T> {
    Value(T),
    Env {
        from_env: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<T>,
    },
}

impl<'de, T: Clone + DeserializeOwned> EnvValue<T> {
    /// get the item
    pub fn value(&self) -> Option<Cow<'_, T>> {
        match self {
            Self::Value(v) => Some(Cow::Borrowed(v)),
            Self::Env { from_env, default } => match std::env::var(from_env) {
                Ok(v) => serde_yaml::from_str(v.as_str())
                    .ok()
                    .map(Cow::Owned)
                    .or_else(|| default.as_ref().map(Cow::Borrowed)),
                Err(_) => default.as_ref().map(Cow::Borrowed),
            },
        }
    }
}

impl<T> EnvValue<T> {
    pub fn map<B, F: FnOnce(T) -> B>(self, func: F) -> EnvValue<B> {
        match self {
            Self::Value(v) => EnvValue::Value(func(v)),
            Self::Env { from_env, default } => EnvValue::Env {
                from_env,
                default: default.map(func),
            },
        }
    }
}

impl<T: Default> Default for EnvValue<T> {
    fn default() -> Self {
        Self::Value(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::to_string;

    #[test]
    fn env_value_serde_test() {
        let val = EnvValue::Value(2);
        assert_eq!(to_string(&val).unwrap(), r#"2"#)
    }
}
