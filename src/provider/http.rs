use std::thread;
use std::time::{Duration, Instant};

use super::{ProviderError, ProviderEvent, ProviderProgress, WaitReason};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct ProviderHttp {
    agent: ureq::Agent,
    user_agent: String,
    last_request: Option<Instant>,
}

impl ProviderHttp {
    pub(super) fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(REQUEST_TIMEOUT))
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .http_status_as_error(false)
            .build();
        Self {
            agent: config.into(),
            user_agent: format!(
                "music-groomer/{} (https://github.com/cark/music-groomer)",
                env!("CARGO_PKG_VERSION")
            ),
            last_request: None,
        }
    }

    pub(super) fn get_json<T: serde::de::DeserializeOwned>(
        &mut self,
        url: &str,
        label: &'static str,
        spacing: Duration,
        deadline: Instant,
        progress: &mut dyn ProviderProgress,
    ) -> Result<T, ProviderError> {
        self.get(url, label, spacing, deadline, progress)?
            .body_mut()
            .read_json()
            .map_err(|error| ProviderError::InvalidResponse(format!("{label}: {error}")))
    }

    pub(super) fn get_bytes(
        &mut self,
        url: &str,
        label: &'static str,
        spacing: Duration,
        deadline: Instant,
        progress: &mut dyn ProviderProgress,
    ) -> Result<Vec<u8>, ProviderError> {
        self.get(url, label, spacing, deadline, progress)?
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|error| ProviderError::Network(format!("{label}: {error}")))
    }

    fn get(
        &mut self,
        url: &str,
        label: &'static str,
        spacing: Duration,
        deadline: Instant,
        progress: &mut dyn ProviderProgress,
    ) -> Result<ureq::http::Response<ureq::Body>, ProviderError> {
        let mut retry_seconds = 1;
        loop {
            self.wait_for_spacing(spacing, progress)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(ProviderError::Network(
                    "transient failures continued for the 60-second retry window".into(),
                ));
            }
            progress.event(ProviderEvent::Requesting(label))?;
            self.last_request = Some(Instant::now());
            let response = self
                .agent
                .get(url)
                .header("User-Agent", &self.user_agent)
                .header("Accept", "application/json, image/*")
                .config()
                .timeout_global(Some(REQUEST_TIMEOUT.min(remaining)))
                .build()
                .call();

            match response {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) if transient_status(response.status().as_u16()) => {
                    let requested = response
                        .headers()
                        .get("Retry-After")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok());
                    let delay = requested.unwrap_or(retry_seconds).clamp(1, 15);
                    retry_seconds = (retry_seconds * 2).min(15);
                    wait_to_retry(delay, deadline, progress)?;
                }
                Ok(response) => {
                    return Err(ProviderError::HttpStatus {
                        operation: label,
                        status: response.status().as_u16(),
                    });
                }
                Err(error) if transient_error(&error) => {
                    let delay = retry_seconds;
                    retry_seconds = (retry_seconds * 2).min(15);
                    wait_to_retry(delay, deadline, progress)?;
                }
                Err(error) => return Err(ProviderError::Network(format!("{label}: {error}"))),
            }
        }
    }

    fn wait_for_spacing(
        &self,
        spacing: Duration,
        progress: &mut dyn ProviderProgress,
    ) -> Result<(), ProviderError> {
        let Some(remaining) = self
            .last_request
            .and_then(|last| spacing.checked_sub(last.elapsed()))
        else {
            return Ok(());
        };
        if !remaining.is_zero() {
            progress.event(ProviderEvent::Waiting {
                seconds: remaining.as_secs().max(1),
                reason: WaitReason::RateLimit,
            })?;
            thread::sleep(remaining);
        }
        Ok(())
    }
}

fn wait_to_retry(
    seconds: u64,
    deadline: Instant,
    progress: &mut dyn ProviderProgress,
) -> Result<(), ProviderError> {
    let delay = Duration::from_secs(seconds);
    if Instant::now()
        .checked_add(delay)
        .is_none_or(|end| end > deadline)
    {
        return Err(ProviderError::Network(
            "transient failures continued for the 60-second retry window".into(),
        ));
    }
    progress.event(ProviderEvent::Waiting {
        seconds,
        reason: WaitReason::Retry,
    })?;
    thread::sleep(delay);
    Ok(())
}

fn transient_status(status: u16) -> bool {
    status == 429 || status >= 500
}

fn transient_error(error: &ureq::Error) -> bool {
    matches!(
        error,
        ureq::Error::Io(_)
            | ureq::Error::Timeout(_)
            | ureq::Error::HostNotFound
            | ureq::Error::ConnectionFailed
    )
}
