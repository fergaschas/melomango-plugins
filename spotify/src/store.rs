//! Settings (Client ID) and tokens, stored as JSON in the plugin's data folder.

use std::{fs, path::PathBuf};

use anyhow::Context;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: u64,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// `$MELOMANGO_DATA_DIR`, or a `data` folder next to the executable when run by hand.
    pub fn open() -> anyhow::Result<Self> {
        let dir = match std::env::var_os("MELOMANGO_DATA_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => std::env::current_exe()?.parent().context("ruta del plugin")?.join("data"),
        };
        fs::create_dir_all(&dir).with_context(|| format!("no se pudo crear {}", dir.display()))?;
        Ok(Self { dir })
    }

    fn read<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        fs::read_to_string(self.dir.join(name)).ok().and_then(|t| serde_json::from_str(&t).ok())
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> anyhow::Result<()> {
        let path = self.dir.join(name);
        fs::write(&path, serde_json::to_string_pretty(value)?)
            .with_context(|| format!("no se pudo guardar {}", path.display()))
    }

    pub fn config(&self) -> Config {
        self.read("config.json").unwrap_or_default()
    }

    pub fn save_config(&self, config: &Config) -> anyhow::Result<()> {
        self.write("config.json", config)
    }

    pub fn token(&self) -> Option<Token> {
        self.read("token.json")
    }

    pub fn save_token(&self, token: &Token) -> anyhow::Result<()> {
        self.write("token.json", token)
    }

    pub fn clear_token(&self) -> anyhow::Result<()> {
        let path = self.dir.join("token.json");
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}
