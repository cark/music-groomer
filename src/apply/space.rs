use std::path::{Path, PathBuf};

const MINIMUM_MARGIN: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaceWarning {
    pub path: PathBuf,
    pub cause: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InsufficientSpace {
    pub path: PathBuf,
    pub required: u64,
    pub available: u64,
}

pub fn required_space(content_bytes: u64) -> u64 {
    let margin = MINIMUM_MARGIN.max(content_bytes / 100);
    content_bytes.saturating_add(margin)
}

pub fn check(path: &Path, required: u64) -> Result<Option<SpaceWarning>, InsufficientSpace> {
    match fs2::available_space(path) {
        Ok(available) if available < required => Err(InsufficientSpace {
            path: path.to_owned(),
            required,
            available,
        }),
        Ok(_) => Ok(None),
        Err(error) => Ok(Some(SpaceWarning {
            path: path.to_owned(),
            cause: error.to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_always_includes_a_small_margin() {
        assert_eq!(required_space(1024), 16 * 1024 * 1024 + 1024);
        assert!(required_space(u64::MAX) > 0);
    }
}
