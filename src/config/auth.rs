use serde::{Deserialize, Serialize};

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
