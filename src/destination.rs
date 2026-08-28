use std::env;
use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::plan::{GroomingPlan, PlanError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DestinationRoot {
    path: PathBuf,
}

impl DestinationRoot {
    pub fn existing(input: &str) -> Result<Self, DestinationError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(DestinationError::Empty);
        }

        let expanded = expand_home(input)?;
        let absolute = if expanded.is_absolute() {
            expanded
        } else {
            env::current_dir()
                .map_err(DestinationError::CurrentDirectory)?
                .join(expanded)
        };
        let path = absolute
            .canonicalize()
            .map_err(|source| DestinationError::Unavailable {
                path: absolute,
                source,
            })?;
        if !path.is_dir() {
            return Err(DestinationError::NotDirectory(path));
        }

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn relocate(&self, plan: GroomingPlan) -> Result<GroomingPlan, DestinationError> {
        let relocated = plan
            .with_destination_root(self.path.clone())
            .map_err(DestinationError::InvalidPlan)?;
        if relocated.destination.try_exists().map_err(|source| {
            DestinationError::CollisionCheck {
                path: relocated.destination.clone(),
                source,
            }
        })? {
            return Err(DestinationError::Collision(relocated.destination));
        }
        Ok(relocated)
    }
}

#[derive(Debug)]
pub enum DestinationError {
    Empty,
    HomeUnavailable,
    CurrentDirectory(io::Error),
    Unavailable { path: PathBuf, source: io::Error },
    NotDirectory(PathBuf),
    Collision(PathBuf),
    CollisionCheck { path: PathBuf, source: io::Error },
    InvalidPlan(PlanError),
}

impl fmt::Display for DestinationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("enter an existing destination directory"),
            Self::HomeUnavailable => {
                formatter.write_str("cannot expand ~ because the home directory is unavailable")
            }
            Self::CurrentDirectory(error) => {
                write!(formatter, "cannot resolve a relative destination: {error}")
            }
            Self::Unavailable { path, source } => {
                write!(formatter, "{} is not available: {source}", path.display())
            }
            Self::NotDirectory(path) => {
                write!(formatter, "{} is not a directory", path.display())
            }
            Self::Collision(path) => write!(
                formatter,
                "the final release path already exists: {}. v0.1 cannot yet complete or rebuild an existing release piece by piece",
                path.display()
            ),
            Self::CollisionCheck { path, source } => write!(
                formatter,
                "cannot check the final release path {}: {source}",
                path.display()
            ),
            Self::InvalidPlan(error) => write!(formatter, "cannot relocate this plan: {error}"),
        }
    }
}

fn expand_home(input: &str) -> Result<PathBuf, DestinationError> {
    expand_home_with(
        input,
        env::var_os("HOME"),
        directories::BaseDirs::new().map(|directories| directories.home_dir().to_owned()),
    )
}

fn expand_home_with(
    input: &str,
    home_environment: Option<OsString>,
    platform_home: Option<PathBuf>,
) -> Result<PathBuf, DestinationError> {
    let home = home_environment
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or(platform_home);
    if input == "~" {
        home.ok_or(DestinationError::HomeUnavailable)
    } else if let Some(rest) = input.strip_prefix("~/") {
        home.map(|home| home.join(rest))
            .ok_or(DestinationError::HomeUnavailable)
    } else {
        Ok(PathBuf::from(input))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_an_existing_directory_and_canonicalizes_it() {
        let root = DestinationRoot::existing(&env::temp_dir().display().to_string())
            .expect("the operating system temp directory exists");

        assert!(root.path().is_absolute());
        assert!(root.path().is_dir());
    }

    #[test]
    fn rejects_a_missing_directory() {
        let missing = env::temp_dir().join(format!(
            "music-groomer-missing-destination-{}",
            std::process::id()
        ));

        let error = DestinationRoot::existing(&missing.display().to_string())
            .expect_err("a missing destination must be rejected");

        assert!(matches!(error, DestinationError::Unavailable { .. }));
    }

    #[test]
    fn non_empty_home_environment_wins_over_platform_home() {
        let expanded = expand_home_with(
            "~/Music",
            Some(OsString::from("E:/home")),
            Some(PathBuf::from("C:/Users/cark")),
        )
        .unwrap();

        assert_eq!(expanded, PathBuf::from("E:/home/Music"));
    }

    #[test]
    fn empty_home_environment_uses_platform_home() {
        let expanded = expand_home_with(
            "~",
            Some(OsString::new()),
            Some(PathBuf::from("C:/Users/cark")),
        )
        .unwrap();

        assert_eq!(expanded, PathBuf::from("C:/Users/cark"));
    }

    #[test]
    fn paths_without_tilde_do_not_require_a_home() {
        let expanded = expand_home_with("relative/music", None, None).unwrap();

        assert_eq!(expanded, PathBuf::from("relative/music"));
    }
}
