use anyhow::anyhow;

pub fn get_var(input: &str) -> anyhow::Result<String> {
    std::env::var(input).map_err(|_| anyhow!("must pass env variable {}", input))
}
