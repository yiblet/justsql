pub fn get_var(input: &str) -> anyhow::Result<String> {
    std::env::var(input).map_err(|_| anyhow!("must pass env variable {}", input))
}

// TODO explain required environment variables
pub fn get_secret() -> anyhow::Result<String> {
    let secret = get_var("AUTH_SECRET")?;
    let key = secret.trim().to_owned();
    Ok(key)
}

pub fn get_cookie_domain() -> anyhow::Result<String> {
    get_var("COOKIE_DOMAIN")
}

pub fn get_cookie_secure() -> bool {
    get_var("COOKIE_SECURE").map_or(true, |res| res.parse().unwrap_or(true))
}

pub fn get_cookie_http_only() -> bool {
    get_var("COOKIE_HTTP_ONLY").map_or(true, |res| res.parse().unwrap_or(true))
}
