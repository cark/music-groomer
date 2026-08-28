use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::provider::DEFAULT_CACHE_MAX_BYTES;

const DEFAULT_RECOVERY_GRACE_DAYS: u64 = 30;
const DEFAULT_RECOVERY_MAX_MIB: u64 = 10 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppConfig {
    pub destination: Option<PathBuf>,
    pub cache_max_mib: Option<u64>,
    pub recovery_grace_days: Option<u64>,
    pub recovery_max_mib: Option<u64>,
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        Self::load_from(&Self::platform_path()?)
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

    pub fn recovery_grace_days(&self) -> Result<u64, ConfigError> {
        match self.recovery_grace_days {
            None => Ok(DEFAULT_RECOVERY_GRACE_DAYS),
            Some(0) => Err(ConfigError::InvalidRecoveryGraceDays),
            Some(days) => Ok(days),
        }
    }

    pub fn recovery_max_bytes(&self) -> Result<u64, ConfigError> {
        let mebibytes = self.recovery_max_mib.unwrap_or(DEFAULT_RECOVERY_MAX_MIB);
        if mebibytes == 0 {
            return Err(ConfigError::InvalidRecoveryLimit);
        }
        mebibytes
            .checked_mul(1024 * 1024)
            .ok_or(ConfigError::InvalidRecoveryLimit)
    }

    pub fn platform_path() -> Result<PathBuf, ConfigError> {
        let directories = directories::ProjectDirs::from("", "", "music-groomer")
            .ok_or(ConfigError::NoPlatformConfigDirectory)?;
        Ok(directories.config_dir().join("config.toml"))
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        self.save_to(&Self::platform_path()?)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        let parent = path
            .parent()
            .ok_or_else(|| ConfigError::InvalidPath(path.to_owned()))?;
        fs::create_dir_all(parent)
            .map_err(|error| ConfigError::Write(path.to_owned(), error.to_string()))?;
        let contents = toml::to_string_pretty(self)
            .map_err(|error| ConfigError::Write(path.to_owned(), error.to_string()))?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .map_err(|error| ConfigError::Write(path.to_owned(), error.to_string()))?;
        temporary
            .write_all(contents.as_bytes())
            .and_then(|()| temporary.as_file_mut().sync_all())
            .map_err(|error| ConfigError::Write(path.to_owned(), error.to_string()))?;
        temporary
            .persist(path)
            .map_err(|error| ConfigError::Write(path.to_owned(), error.error.to_string()))?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum ConfigError {
    NoPlatformConfigDirectory,
    Read(PathBuf, std::io::Error),
    Parse(PathBuf, toml::de::Error),
    InvalidCacheLimit,
    InvalidRecoveryGraceDays,
    InvalidRecoveryLimit,
    InvalidPath(PathBuf),
    Write(PathBuf, String),
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
            Self::InvalidRecoveryGraceDays => {
                formatter.write_str("recovery_grace_days must be a positive whole number")
            }
            Self::InvalidRecoveryLimit => formatter
                .write_str("recovery_max_mib must be a positive whole number that fits in bytes"),
            Self::InvalidPath(path) => write!(
                formatter,
                "configuration path has no parent directory: {}",
                path.display()
            ),
            Self::Write(path, error) => {
                write!(formatter, "cannot save {}: {error}", path.display())
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
        assert_eq!(config.recovery_grace_days().unwrap(), 30);
        assert_eq!(
            config.recovery_max_bytes().unwrap(),
            10 * 1024 * 1024 * 1024
        );
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

    #[test]
    fn destination_round_trips_through_an_atomic_save() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("config/config.toml");
        let config = AppConfig {
            destination: Some(PathBuf::from("/media/music")),
            cache_max_mib: Some(12),
            recovery_grace_days: Some(45),
            recovery_max_mib: Some(20 * 1024),
        };

        config.save_to(&path).unwrap();

        assert_eq!(AppConfig::load_from(&path).unwrap(), config);
    }

    #[test]
    fn recovery_preferences_are_configurable() {
        let temporary = TempDir::new().unwrap();
        let path = temporary.path().join("config.toml");
        fs::write(
            &path,
            "recovery_grace_days = 90\nrecovery_max_mib = 20480\n",
        )
        .unwrap();
        let config = AppConfig::load_from(&path).unwrap();

        assert_eq!(config.recovery_grace_days().unwrap(), 90);
        assert_eq!(
            config.recovery_max_bytes().unwrap(),
            20 * 1024 * 1024 * 1024
        );
    }

    #[test]
    fn zero_recovery_preferences_are_rejected() {
        let grace = AppConfig {
            recovery_grace_days: Some(0),
            ..AppConfig::default()
        };
        assert!(matches!(
            grace.recovery_grace_days(),
            Err(ConfigError::InvalidRecoveryGraceDays)
        ));

        let limit = AppConfig {
            recovery_max_mib: Some(0),
            ..AppConfig::default()
        };
        assert!(matches!(
            limit.recovery_max_bytes(),
            Err(ConfigError::InvalidRecoveryLimit)
        ));
    }

    #[test]
    fn overflowing_recovery_limit_is_rejected() {
        let config = AppConfig {
            recovery_max_mib: Some(u64::MAX),
            ..AppConfig::default()
        };

        assert!(matches!(
            config.recovery_max_bytes(),
            Err(ConfigError::InvalidRecoveryLimit)
        ));
    }
}
