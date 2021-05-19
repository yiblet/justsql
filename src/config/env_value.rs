use std::borrow::Cow;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum EnvValue<T> {
    Value(T),
    Env {
        #[serde(with = "from_env_serde")]
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

mod from_env_serde {
    use serde::{de, Deserialize, Deserializer, Serializer};

    // serialize the secret key
    pub fn serialize<S>(kind: &String, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(format!("${}", kind).as_str())
    }

    pub fn deserialize<'de, D>(des: D) -> Result<String, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value: String = Deserialize::deserialize(des)?;
        if value.chars().next() != Some('$') {
            Err(de::Error::custom("from_env value must start with '$'"))
        } else {
            value
                .get(1..)
                .ok_or_else(|| de::Error::custom("from_env value must start with '$'"))
                .map(|val| val.to_string())
        }
    }
} /* from_env_serde */

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_str, to_string};

    #[test]
    fn env_value_serde_test() {
        let val = EnvValue::Value(2);
        assert_eq!(to_string(&val).unwrap(), r#"2"#);
        let val = EnvValue::<()>::Env {
            from_env: "test".to_string(),
            default: None,
        };
        assert_eq!(to_string(&val).unwrap(), r#"{"from_env":"$test"}"#);
        assert_eq!(
            &val,
            &from_str::<EnvValue<()>>(r#"{"from_env":"$test"}"#).unwrap()
        )
    }
}
