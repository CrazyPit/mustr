use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::store::{atomic_write, Store};

/// Global config at `~/.mustr/config.toml`. Currently empty — kept as the home
/// for future settings (theme, default agent, …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {}

impl Config {
    /// Loads the config, or the empty default when the file is absent.
    pub fn load(store: &Store) -> Result<Config> {
        let path = store.config_path();
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
            Err(e) => return Err(Error::io(&path, e)),
        };
        toml::from_str(&raw).map_err(|source| Error::TomlRead { path, source })
    }

    /// Writes the config atomically to `config.toml`.
    pub fn save(&self, store: &Store) -> Result<()> {
        let path = store.config_path();
        let raw = toml::to_string_pretty(self).map_err(|source| Error::TomlWrite {
            path: path.clone(),
            source,
        })?;
        atomic_write(&path, &raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        assert_eq!(Config::load(&store).unwrap(), Config::default());
    }

    #[test]
    fn save_then_load_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        Config::default().save(&store).unwrap();
        assert_eq!(Config::load(&store).unwrap(), Config::default());
    }

    #[test]
    fn load_invalid_toml_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        std::fs::write(store.config_path(), "= not valid").unwrap();
        assert!(Config::load(&store).is_err());
    }
}
