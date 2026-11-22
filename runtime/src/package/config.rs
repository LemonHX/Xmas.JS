use color_eyre::eyre::Result;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::read_to_string;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default)]
    pub registry: Vec<Registry>,
    #[serde(default)]
    pub disallow_install_scripts: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    pub url: String,
    pub scope: Option<String>,
    pub auth: Option<RegistryAuth>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
pub enum RegistryAuth {
    Token {
        token: AuthSource,
    },
    Basic {
        username: AuthSource,
        #[serde(default)]
        password: Option<AuthSource>,
    },
}

impl RegistryAuth {
    /// The one header this credential adds, as (name, value).
    pub fn header(&self) -> Result<(&'static str, String)> {
        use base64::Engine as _;
        Ok(match self {
            RegistryAuth::Token { token } => {
                ("authorization", format!("Bearer {}", token.read_token()?))
            }
            RegistryAuth::Basic { username, password } => {
                let username = username.read_token()?;
                let password = password.as_ref().map(|x| x.read_token()).transpose()?;
                let pair = match password {
                    Some(password) => format!("{username}:{password}"),
                    None => format!("{username}:"),
                };
                (
                    "authorization",
                    format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode(pair)
                    ),
                )
            }
        })
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "snake_case")]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
pub enum AuthSource {
    Inline(String),
    FromEnv { from_env: String },
}

impl AuthSource {
    #[tracing::instrument]
    pub fn read_token(&self) -> Result<String> {
        match self {
            AuthSource::Inline(x) => Ok(x.clone()),
            AuthSource::FromEnv { from_env } => Ok(env::var(from_env)?),
        }
    }
}

pub fn read_config() -> Result<Config> {
    let config = read_to_string("xmas.toml");
    if let Ok(config) = config {
        Ok(toml::from_str(&config)?)
    } else {
        Ok(Config::default())
    }
}
