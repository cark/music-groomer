use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::provider::DEFAULT_CACHE_MAX_BYTES;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub destination: Option<PathBuf>,
    pub cache_max_mib: Option<u64>,
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        let directories = directories::ProjectDirs::from("", "", "music-groomer")
            .ok_or(ConfigError::NoPlatformConfigDirectory)?;
        Self::load_from(&directories.config_dir().join("config.toml"))
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let contents = match fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(ConfigError::Read(path.to_owned(), error)),
        };
        toml::from_str(&contents).map_err(|error| ConfigError::Parse(path.to_owned(), error))
    }

    pub fn cache_max_bytes(&self) -> Result<u64, ConfigError> {
        match self.cache_max_mib {
            None => Ok(DEFAULT_CACHE_MAX_BYTES),
            Some(0) => Err(ConfigError::InvalidCacheLimit),
            Some(mebibytes) => mebibytes
                .checked_mul(1024 * 1024)
                .ok_or(ConfigError::InvalidCacheLimit),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    NoPlatformConfigDirectory,
    Read(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    InvalidCacheLimit,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPlatformConfigDirectory => {
                formatter.write_str("this platform has no user configuration directory")
            }
            Self::Read(path, error) => write!(formatter, "cannot read {}: {error}", path.display()),
            Self::Parse(path, error) => {
                write!(formatter, "cannot parse {}: {error}", path.display())
            }
            Self::InvalidCacheLimit => {
                formatter.write_str("cache_max_mib must be a positive whole number")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn missing_configuration_uses_bounded_default() {
        let temporary = TempDir::new().unwrap();
        let config = AppConfig::load_from(&temporary.path().join("missing.toml")).unwrap();

        assert_eq!(config.cache_max_bytes().unwrap(), 256 * 1024 * 1024);
    }

    #[test]
    fn cache_limit_is_configurable_in_mebibytes() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("config.toml");
        fs::write(&path, "cache_max_mib = 12\n").unwrap();

        assert_eq!(
            AppConfig::load_from(&path)
                .unwrap()
                .cache_max_bytes()
                .unwrap(),
            12 * 1024 * 1024
        );
    }
}
