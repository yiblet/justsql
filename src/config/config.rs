use std::{borrow::Cow, env, fs::File, path::Path};

use actix_web::http;
use anyhow::Context;
use serde::{Deserialize, Serialize};

use super::{env_value::EnvValue, secret::Secret};

// TODO add assume_null_if_missing field
#[derive(Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub database: Database,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Secret>,
    #[serde(default)]
    pub cookie: Cookie,
    #[serde(default)]
    pub cors: Cors,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Database {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<EnvValue<String>>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Cors {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<EnvValue<String>>>,
}

impl Cors {
    pub fn cors(&self) -> actix_cors::Cors {
        let mut cors = actix_cors::Cors::default()
            .allowed_methods(vec![
                http::Method::GET,
                http::Method::POST,
                http::Method::OPTIONS,
            ])
            .max_age(Some(600));

        for origin in self
            .allowed_origins
            .iter()
            .flat_map(|vec| vec.iter())
            .filter_map(|val| val.value())
        {
            cors = cors.allowed_origin(origin.as_ref().as_str());
        }
        cors
    }
}

#[derive(Serialize, Deserialize)]
pub struct Cookie {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<EnvValue<String>>,
    #[serde(default = "true_env_value")]
    pub http_only: EnvValue<bool>,
    #[serde(default = "true_env_value")]
    pub secure: EnvValue<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<EnvValue<String>>,
}

impl Cookie {
    pub fn build<'c, N, V>(&self, name: N, value: V) -> actix_web::cookie::Cookie<'c>
    where
        N: Into<Cow<'c, str>>,
        V: Into<Cow<'c, str>>,
    {
        let mut builder = actix_web::cookie::Cookie::build(name, value);
        if let Some(domain) = self.domain() {
            builder = builder.domain(domain.into_owned())
        }
        if let Some(path) = self.path() {
            builder = builder.path(path.into_owned())
        }

        let cookie = builder
            .secure(self.secure())
            .http_only(self.http_only())
            .finish();

        cookie
    }

    pub fn domain(&self) -> Option<Cow<String>> {
        self.domain.as_ref().and_then(|env_value| env_value.value())
    }

    pub fn path(&self) -> Option<Cow<String>> {
        self.path.as_ref().and_then(|env_value| env_value.value())
    }

    pub fn secure(&self) -> bool {
        // by default leave insecure for users who do not use https
        self.secure.value().as_ref().map_or(false, |v| *v.as_ref())
    }

    pub fn http_only(&self) -> bool {
        self.http_only
            .value()
            .as_ref()
            .map_or(true, |v| *v.as_ref())
    }
}

fn true_env_value() -> EnvValue<bool> {
    EnvValue::Value(true)
}

impl Default for Cookie {
    fn default() -> Self {
        Cookie {
            domain: None,
            http_only: EnvValue::Value(true),
            secure: EnvValue::Value(false),
            path: None,
        }
    }
}

impl Config {
    /// read config from env
    pub fn read_config<P: AsRef<Path>>(file_path_opt: Option<P>) -> anyhow::Result<Config> {
        let config_res = match file_path_opt {
            Some(path) => Self::read_config_from_file_path(path),
            None => Self::read_config_from_directory_parents(),
        };
        config_res.context("failed to read config file")
    }

    pub fn read_config_from_file_path<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let file = File::open(path)?;
        let mut config: Config = serde_yaml::from_reader(file)?;
        if let Some(secret) = config.auth.as_mut() {
            secret.post_process()?
        }
        Ok(config)
    }

    fn read_config_from_directory_parents() -> anyhow::Result<Self> {
        let mut cur = env::current_dir()?;
        loop {
            // check first if the .yaml file exists
            cur.push("justsql.config.yaml");
            let is_file = cur.as_path().metadata().map_or(false, |m| m.is_file());
            if is_file {
                break;
            }
            cur.pop();

            // else check if the .yml file exists
            cur.push("justsql.config.yml");
            let is_file = cur.as_path().metadata().map_or(false, |m| m.is_file());
            if is_file {
                break;
            }
            cur.pop();

            if !cur.pop() {
                return Err(anyhow!(
                    "could not find or open a justsql.config.yaml file in current or parent directories"
                ));
            }
        }

        let file = File::open(&cur)?;
        let mut config: Config = serde_yaml::from_reader(file)?;
        if let Some(secret) = config.auth.as_mut() {
            secret.post_process()?
        }
        Ok(config)
    }
}
