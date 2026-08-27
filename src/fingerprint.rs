use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const FINGERPRINT_AUDIO_SECONDS: u32 = 120;
pub const FINGERPRINT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioFingerprint {
    pub duration_seconds: u32,
    pub value: String,
}

pub trait AudioFingerprinter {
    fn calculate(
        &mut self,
        audio: &Path,
        progress: &mut dyn FingerprintProgress,
    ) -> Result<AudioFingerprint, FingerprintError>;
}

impl<T: AudioFingerprinter + ?Sized> AudioFingerprinter for &mut T {
    fn calculate(
        &mut self,
        audio: &Path,
        progress: &mut dyn FingerprintProgress,
    ) -> Result<AudioFingerprint, FingerprintError> {
        (**self).calculate(audio, progress)
    }
}

pub trait FingerprintProgress {
    fn calculating(&mut self, audio: &Path) -> Result<(), FingerprintError>;
}

impl FingerprintProgress for () {
    fn calculating(&mut self, _audio: &Path) -> Result<(), FingerprintError> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct FpcalcFingerprinter {
    executable: PathBuf,
    timeout: Duration,
}

impl Default for FpcalcFingerprinter {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("fpcalc"),
            timeout: FINGERPRINT_TIMEOUT,
        }
    }
}

impl AudioFingerprinter for FpcalcFingerprinter {
    fn calculate(
        &mut self,
        audio: &Path,
        progress: &mut dyn FingerprintProgress,
    ) -> Result<AudioFingerprint, FingerprintError> {
        progress.calculating(audio)?;
        let mut child = Command::new(&self.executable)
            .arg("-json")
            .arg("-length")
            .arg(FINGERPRINT_AUDIO_SECONDS.to_string())
            .arg(audio)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    FingerprintError::Unavailable
                } else {
                    FingerprintError::Start(error.to_string())
                }
            })?;

        let deadline = Instant::now() + self.timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(FingerprintError::Timeout);
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(FingerprintError::Wait(error.to_string()));
                }
            }
        };

        let mut stdout = Vec::new();
        if let Some(mut output) = child.stdout.take() {
            output
                .read_to_end(&mut stdout)
                .map_err(|error| FingerprintError::Output(error.to_string()))?;
        }
        let mut stderr = String::new();
        if let Some(mut output) = child.stderr.take() {
            output
                .read_to_string(&mut stderr)
                .map_err(|error| FingerprintError::Output(error.to_string()))?;
        }
        if !status.success() {
            return Err(FingerprintError::Failed {
                status: status.code(),
                message: stderr.trim().to_owned(),
            });
        }
        let output: FpcalcOutput = serde_json::from_slice(&stdout)
            .map_err(|error| FingerprintError::InvalidOutput(error.to_string()))?;
        if output.fingerprint.trim().is_empty()
            || !output.duration.is_finite()
            || output.duration <= 0.0
            || output.duration > f64::from(u32::MAX)
        {
            return Err(FingerprintError::InvalidOutput(
                "fpcalc returned an empty fingerprint or zero duration".into(),
            ));
        }
        Ok(AudioFingerprint {
            duration_seconds: output.duration.round() as u32,
            value: output.fingerprint,
        })
    }
}

#[derive(Debug)]
pub enum FingerprintError {
    Unavailable,
    Start(String),
    Timeout,
    Wait(String),
    Output(String),
    InvalidOutput(String),
    Failed {
        status: Option<i32>,
        message: String,
    },
    Progress(String),
}

impl fmt::Display for FingerprintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("fpcalc is not available in PATH"),
            Self::Start(error) => write!(formatter, "cannot start fpcalc: {error}"),
            Self::Timeout => formatter.write_str("fpcalc exceeded its 60-second timeout"),
            Self::Wait(error) => write!(formatter, "cannot wait for fpcalc: {error}"),
            Self::Output(error) => write!(formatter, "cannot read fpcalc output: {error}"),
            Self::InvalidOutput(error) => {
                write!(formatter, "fpcalc returned invalid data: {error}")
            }
            Self::Failed { status, message } => {
                write!(formatter, "fpcalc failed")?;
                if let Some(status) = status {
                    write!(formatter, " with status {status}")?;
                }
                if !message.is_empty() {
                    write!(formatter, ": {message}")?;
                }
                Ok(())
            }
            Self::Progress(error) => {
                write!(formatter, "cannot report fingerprint progress: {error}")
            }
        }
    }
}

impl std::error::Error for FingerprintError {}

#[derive(Deserialize)]
struct FpcalcOutput {
    duration: f64,
    fingerprint: String,
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn parses_reference_fpcalc_json_shape() {
        let output: FpcalcOutput =
            serde_json::from_str(r#"{"duration": 123, "fingerprint": "AQAAE0mUaEkSRZEGAA"}"#)
                .unwrap();

        assert_eq!(output.duration, 123.0);
        assert_eq!(output.fingerprint, "AQAAE0mUaEkSRZEGAA");
    }

    #[cfg(unix)]
    #[test]
    fn invokes_the_helper_with_a_literal_path_and_expected_bounds() {
        let temporary = TempDir::new().unwrap();
        let helper = temporary.path().join("fake-fpcalc");
        fs::write(
            &helper,
            "#!/bin/sh\n[ \"$1\" = -json ] || exit 10\n[ \"$2\" = -length ] || exit 11\n[ \"$3\" = 120 ] || exit 12\n[ \"${4##*/}\" = 'audio; not a shell.flac' ] || exit 13\nprintf '%s' '{\"duration\":180,\"fingerprint\":\"fingerprint-value\"}'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&helper).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&helper, permissions).unwrap();
        let audio = temporary.path().join("audio; not a shell.flac");
        fs::write(&audio, b"fixture").unwrap();
        // The helper sees the path only as one process argument; shell metacharacters are inert.
        let mut fingerprinter = FpcalcFingerprinter {
            executable: helper,
            timeout: Duration::from_secs(2),
        };

        let result = fingerprinter.calculate(&audio, &mut ()).unwrap();

        assert_eq!(result.duration_seconds, 180);
        assert_eq!(result.value, "fingerprint-value");
    }

    #[cfg(unix)]
    #[test]
    fn terminates_a_helper_that_exceeds_the_deadline() {
        let temporary = TempDir::new().unwrap();
        let helper = temporary.path().join("slow-fpcalc");
        fs::write(&helper, "#!/bin/sh\nexec sleep 5\n").unwrap();
        let mut permissions = fs::metadata(&helper).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&helper, permissions).unwrap();
        let mut fingerprinter = FpcalcFingerprinter {
            executable: helper,
            timeout: Duration::from_millis(150),
        };
        let started = Instant::now();

        let error = fingerprinter
            .calculate(temporary.path().join("audio.flac").as_path(), &mut ())
            .unwrap_err();

        assert!(matches!(error, FingerprintError::Timeout));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
}
