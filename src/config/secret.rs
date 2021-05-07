use std::{
    borrow::Cow,
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::binding::Binding;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::{env_value::EnvValue, AuthClaims};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Secret {
    pub algorithm: Algorithm,
    #[serde(flatten)]
    #[serde(with = "secret_kind_serde")]
    pub kind: SecretKind,

    #[serde(skip)] // TODO store keys directly instead
    file_locs: BTreeMap<PathBuf, Vec<u8>>,
}

fn get_val<'a, T: Clone + DeserializeOwned>(
    val: &'a EnvValue<T>,
    name: &str,
) -> anyhow::Result<Cow<'a, T>> {
    let val = val
        .value()
        .ok_or_else(|| anyhow!("could not get {}", name))?;
    Ok(val)
}

impl Secret {
    pub fn encode<A: Serialize>(&self, claims: &A, exp: u64) -> anyhow::Result<String> {
        let token = jsonwebtoken::encode(
            &jsonwebtoken::Header::default(),
            &AuthClaims {
                iss: Some("justsql".to_owned()),
                exp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + exp,
                claims,
            },
            &self.encoding_key()?,
        )?;
        Ok(token)
    }

    pub fn decode(&self, token: &str) -> anyhow::Result<AuthClaims<BTreeMap<String, Binding>>> {
        let decoding_key = self.decoding_key()?;
        let data =
            jsonwebtoken::decode(token, &decoding_key, &jsonwebtoken::Validation::default())?;
        Ok(data.claims)
    }

    fn get_file_contents<'a>(&'a self, path: &Path) -> anyhow::Result<&'a [u8]> {
        let file_contents = self
            .file_locs
            .get(path)
            .ok_or_else(|| anyhow!("could not find file at {:?}", path.as_os_str()))?;
        Ok(file_contents.as_slice())
    }

    /// get the encoding key
    fn encoding_key(&self) -> anyhow::Result<EncodingKey> {
        match &self.kind {
            SecretKind::Symmetric { secret } => match secret {
                SecretKey::FromFile(file) => {
                    let file_contents =
                        self.get_file_contents(get_val(file, "secret_key file name")?.as_path())?;
                    let decoded = base64::decode(file_contents);
                    let contents = decoded.as_ref().map_or(file_contents, |val| val.as_slice());
                    Ok(EncodingKey::from_secret(contents))
                }
                SecretKey::Base64(val) => {
                    let val = get_val(val, "base64 value")?;
                    Ok(EncodingKey::from_base64_secret(val.as_str())?)
                }
            },
            SecretKind::Assymmetric { encoding, .. } => {
                let create_encoding_key = match self.algorithm {
                    Algorithm::RS256
                    | Algorithm::RS384
                    | Algorithm::RS512
                    | Algorithm::PS256
                    | Algorithm::PS384
                    | Algorithm::PS512 => EncodingKey::from_rsa_pem,
                    Algorithm::ES256 | Algorithm::ES384 => EncodingKey::from_ec_pem,
                    _ => Err(anyhow!("HS algorithm only accepts symmetric base64 keys"))?,
                };
                match encoding
                    .as_ref()
                    .ok_or_else(|| anyhow!("encoding key is not set"))?
                {
                    SecretKey::FromFile(file) => {
                        let file_contents = self.get_file_contents(
                            get_val(file, "encoding key file name")?.as_path(),
                        )?;
                        Ok(create_encoding_key(file_contents)?)
                    }
                    SecretKey::Base64(val) => {
                        let val = val
                            .value()
                            .ok_or_else(|| anyhow!("could not get secret_key base64 value"))?;
                        let contents = base64::decode(val.as_str())?;
                        Ok(create_encoding_key(contents.as_slice())?)
                    }
                }
            }
        }
    }

    /// get the decoding key
    fn decoding_key(&self) -> anyhow::Result<DecodingKey> {
        match &self.kind {
            SecretKind::Symmetric { secret } => match secret {
                SecretKey::FromFile(file) => {
                    let file_contents =
                        self.get_file_contents(get_val(file, "secret_key file name")?.as_path())?;
                    let decoded = base64::decode(file_contents);
                    let contents = decoded.as_ref().map_or(file_contents, |val| val.as_slice());
                    Ok(DecodingKey::from_secret(contents).into_static())
                }
                SecretKey::Base64(val) => {
                    let val = get_val(val, "base64 value")?;
                    Ok(DecodingKey::from_base64_secret(val.as_str())?)
                }
            },
            SecretKind::Assymmetric { decoding, .. } => {
                let create_decoding_key = match self.algorithm {
                    Algorithm::RS256
                    | Algorithm::RS384
                    | Algorithm::RS512
                    | Algorithm::PS256
                    | Algorithm::PS384
                    | Algorithm::PS512 => DecodingKey::from_rsa_pem,
                    Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem,
                    _ => Err(anyhow!("HS algorithm only accepts symmetric base64 keys"))?,
                };

                match decoding {
                    SecretKey::FromFile(file) => {
                        let file_contents = self.get_file_contents(
                            get_val(file, "decoding key file name")?.as_path(),
                        )?;
                        Ok(create_decoding_key(file_contents)?.into_static())
                    }
                    SecretKey::Base64(val) => {
                        let val = val
                            .value()
                            .ok_or_else(|| anyhow!("could not get secret_key base64 value"))?;
                        let contents = base64::decode(val.as_str())?;
                        Ok(create_decoding_key(contents.as_slice())?.into_static())
                    }
                }
            }
        }
    }

    pub fn post_process(&mut self) -> anyhow::Result<()> {
        if self.is_symmetric_algorithm() != matches!(self.kind, SecretKind::Symmetric { .. }) {
            Err(anyhow!(
                "algorithm requires symmetric secret but was given assymetric key(s), either change the algorithm
                to HS512, HS384, or HS256 or use put your key in secret_key_base64"
            ))?
        }
        if matches!(
            self.kind,
            SecretKind::Symmetric {
                secret: SecretKey::FromFile(_)
            }
        ) {
            Err(anyhow!(
                "cannot pull secret_key from file pass it through secret_key_base64"
            ))?
        }

        let secrets: Vec<&SecretKey> = match &self.kind {
            SecretKind::Symmetric { secret } => vec![secret],
            SecretKind::Assymmetric {
                encoding: Some(encoding),
                decoding,
            } => vec![encoding, decoding],
            SecretKind::Assymmetric {
                encoding: None,
                decoding,
            } => vec![decoding],
        };

        let file_locs: std::io::Result<BTreeMap<PathBuf, Vec<u8>>> = secrets
            .into_iter()
            .filter_map(|secret: &SecretKey| match secret {
                SecretKey::FromFile(from_file) => from_file.value(),
                _ => None,
            })
            .map(|path| {
                let mut vec = vec![];
                let mut file = std::fs::File::open(path.as_path())?;
                file.read_to_end(&mut vec)?;
                Ok((path.into_owned(), vec))
            })
            .collect();

        self.file_locs = file_locs?;
        Ok(())
    }

    pub fn is_symmetric_algorithm(&self) -> bool {
        match self.algorithm {
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => true,
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum SecretKind {
    Symmetric {
        secret: SecretKey,
    },
    Assymmetric {
        encoding: Option<SecretKey>,
        decoding: SecretKey,
    },
}

#[derive(Debug, PartialEq)]
pub enum SecretKey {
    FromFile(EnvValue<PathBuf>),
    Base64(EnvValue<String>),
}

mod secret_kind_serde {
    use std::collections::BTreeMap;

    use serde::{de, ser::SerializeMap, Deserialize, Deserializer, Serializer};

    use crate::config::env_value::EnvValue;

    use super::{SecretKey, SecretKind};

    // serialize the secret key
    pub fn serialize<S>(kind: &SecretKind, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        fn field_serializer<S: Serializer>(
            key: &str,
            secret: &SecretKey,
            ser: &mut S::SerializeMap,
        ) -> Result<(), S::Error> {
            let variant = match secret {
                SecretKey::FromFile(_) => "from_file",
                SecretKey::Base64(_) => "base64",
            };
            let key = format!("{}_{}", key, variant);
            match secret {
                SecretKey::FromFile(val) => ser.serialize_entry(key.as_str(), val),
                SecretKey::Base64(val) => ser.serialize_entry(key.as_str(), val),
            }
        }

        match kind {
            SecretKind::Symmetric { secret } => {
                let mut ser = serializer.serialize_map(Some(1))?;
                field_serializer::<S>("secret_key", secret, &mut ser)?;
                ser.end()
            }
            SecretKind::Assymmetric { decoding, encoding } => {
                let mut ser =
                    serializer.serialize_map(Some(1 + encoding.as_ref().map_or(0, |_| 1)))?;
                field_serializer::<S>("decoding_key", decoding, &mut ser)?;
                if let Some(encoding) = encoding {
                    field_serializer::<S>("encoding_key", encoding, &mut ser)?;
                }
                ser.end()
            }
        }
    }

    pub fn deserialize<'de, D>(des: D) -> Result<SecretKind, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map: BTreeMap<String, EnvValue<String>> = Deserialize::deserialize(des)?;
        let mut secret = None;
        let mut encoding = None;
        let mut decoding = None;

        for (key, value) in map.into_iter() {
            let category = if key.starts_with("secret_key") {
                "secret"
            } else if key.starts_with("encoding_key") {
                "encoding"
            } else if key.starts_with("decoding_key") {
                "decoding"
            } else {
                continue;
            };

            let is_base64 = if key.ends_with("from_file") {
                false
            } else if key.ends_with("base64") {
                true
            } else {
                continue;
            };

            let secret_key = if is_base64 {
                SecretKey::Base64(value)
            } else {
                SecretKey::FromFile(value.map(|string| string.into()))
            };

            let old = match category {
                "secret" => secret.replace(secret_key),
                "encoding" => encoding.replace(secret_key),
                "decoding" => decoding.replace(secret_key),
                _ => Err(de::Error::custom("unexpected category"))?,
            };

            if old.is_some() {
                Err(de::Error::custom(format!(
                    "duplicate key starting with {}",
                    category
                )))?
            }
        }

        let secret_kind = match (secret, decoding, encoding) {
        (Some(secret), None, None) => SecretKind::Symmetric { secret },
        (None, Some(decoding), encoding) => SecretKind::Assymmetric {
            decoding,
            encoding,
        },
        _ => 
            Err(de::Error::custom(
            "for the HS algorithms (HS256, HS384, HS512) the field secret_key_base64, or secret_key_from_file is expected; for the other algorithms field decoding_key_base64 or decoding_key_from_file, and the field field encoding_key_base64 or encoding_key_from_file is expected. 
            ",
        ))?
            ,
    };

        Ok(secret_kind)
    }
} /* secret_kind_serde */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_serde_test() {
        let secret = Secret {
            algorithm: Algorithm::HS256,
            kind: SecretKind::Symmetric {
                secret: SecretKey::Base64(EnvValue::Value("testing".to_string())),
            },
            file_locs: Default::default(),
        };

        let data = serde_json::to_string(&secret).unwrap();
        assert_eq!(
            &data,
            "{\"algorithm\":\"HS256\",\"secret_key_base64\":\"testing\"}"
        );

        let reverse = serde_json::from_str(data.as_str()).unwrap();
        assert_eq!(&secret, &reverse);
    }
}
