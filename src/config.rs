use serde::{Deserialize, Serialize};

use crate::agent::AgentKind;
use crate::error::{Error, Result};
use crate::store::{atomic_write, Store};

/// Global config at `~/.mustr/config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Fallback agent kind for `agent open` when neither `--type` nor the
    /// project sets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,
    /// Pre-authorize each launched agent's workspace so it skips its trust
    /// prompt — only for agents that expose a flag for it (codex, cursor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust_workspaces: Option<bool>,
}

impl Config {
    /// The settable config keys, for listing and help.
    pub const KEYS: &'static [&'static str] = &["default_agent", "trust_workspaces"];

    /// Current value of `key` as a display string, or None when unset.
    pub fn get(&self, key: &str) -> Result<Option<String>> {
        match key {
            "default_agent" => Ok(self.default_agent.clone()),
            "trust_workspaces" => Ok(self.trust_workspaces.map(|b| b.to_string())),
            _ => Err(Error::UnknownConfigKey { key: key.into() }),
        }
    }

    /// Sets `key` to `value`, validating per key.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "default_agent" => {
                if AgentKind::parse(value).is_none() {
                    return Err(Error::InvalidConfigValue {
                        key: key.into(),
                        value: value.into(),
                        allowed: "claude, codex, cursor",
                    });
                }
                self.default_agent = Some(value.into());
            }
            "trust_workspaces" => {
                let b = match value {
                    "true" => true,
                    "false" => false,
                    _ => {
                        return Err(Error::InvalidConfigValue {
                            key: key.into(),
                            value: value.into(),
                            allowed: "true, false",
                        })
                    }
                };
                self.trust_workspaces = Some(b);
            }
            _ => return Err(Error::UnknownConfigKey { key: key.into() }),
        }
        Ok(())
    }

    /// Clears `key`, reverting it to its default.
    pub fn unset(&mut self, key: &str) -> Result<()> {
        match key {
            "default_agent" => self.default_agent = None,
            "trust_workspaces" => self.trust_workspaces = None,
            _ => return Err(Error::UnknownConfigKey { key: key.into() }),
        }
        Ok(())
    }

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

/// Per-project config at `~/.mustr/projects/<project>/config.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    /// Default agent kind for `mustr agent open` when none is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,
}

impl ProjectConfig {
    /// Loads the project config, or the empty default when absent.
    pub fn load(store: &Store, project: &str) -> Result<ProjectConfig> {
        let path = store.project_config_path(project);
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProjectConfig::default())
            }
            Err(e) => return Err(Error::io(&path, e)),
        };
        toml::from_str(&raw).map_err(|source| Error::TomlRead { path, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_default_agent_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        std::fs::create_dir_all(store.project_dir("proj")).unwrap();
        assert_eq!(
            ProjectConfig::load(&store, "proj").unwrap().default_agent,
            None
        );

        std::fs::write(
            store.project_config_path("proj"),
            "default_agent = \"claude\"\n",
        )
        .unwrap();
        assert_eq!(
            ProjectConfig::load(&store, "proj")
                .unwrap()
                .default_agent
                .as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn get_set_unset_with_validation() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path());
        store.ensure().unwrap();
        let mut cfg = Config::default();

        assert_eq!(cfg.get("default_agent").unwrap(), None);

        cfg.set("default_agent", "codex").unwrap();
        assert_eq!(cfg.get("default_agent").unwrap().as_deref(), Some("codex"));
        assert!(matches!(
            cfg.set("default_agent", "opencode"),
            Err(Error::InvalidConfigValue { .. })
        ));

        cfg.set("trust_workspaces", "true").unwrap();
        assert_eq!(
            cfg.get("trust_workspaces").unwrap().as_deref(),
            Some("true")
        );
        assert!(matches!(
            cfg.set("trust_workspaces", "maybe"),
            Err(Error::InvalidConfigValue { .. })
        ));

        assert!(matches!(
            cfg.get("nope"),
            Err(Error::UnknownConfigKey { .. })
        ));
        assert!(matches!(
            cfg.set("nope", "x"),
            Err(Error::UnknownConfigKey { .. })
        ));

        cfg.unset("default_agent").unwrap();
        assert_eq!(cfg.get("default_agent").unwrap(), None);

        cfg.save(&store).unwrap();
        assert_eq!(Config::load(&store).unwrap().trust_workspaces, Some(true));
    }

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
