use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{DecodingKey, EncodingKey};
use serde::{Deserialize, Serialize};

use crate::{binding::Binding, row_type::RowType, util::get_secret};

#[derive(Deserialize, Serialize)]
pub struct AuthClaims<A> {
    /// issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,

    /// expiration date in seconds since epoch (utc)
    pub exp: u64,

    /// additional claims
    #[serde(flatten)]
    pub claims: A,
}

pub fn encode<A: Serialize>(claims: &A, exp: u64) -> anyhow::Result<String> {
    let token = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &AuthClaims {
            iss: Some("justsql".to_owned()),
            exp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + exp,
            claims,
        },
        &EncodingKey::from_secret(get_secret()?.as_bytes()),
    )?;
    Ok(token)
}

pub fn decode(token: &str) -> anyhow::Result<AuthClaims<BTreeMap<String, Binding>>> {
    let secret = &get_secret()?;
    let data = jsonwebtoken::decode(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    )?;
    Ok(data.claims)
}
