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
                    let retry_after = response
                        .headers()
                        .get("Retry-After")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok());
                    let delay = retry_delay(retry_after, retry_seconds);
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

fn retry_delay(retry_after: Option<u64>, fallback: u64) -> u64 {
    retry_after.unwrap_or(fallback).clamp(1, 15)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn direct_requests_send_the_meaningful_user_agent() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sent, received) = mpsc::channel();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let length = stream.read(&mut request).unwrap();
            sent.send(String::from_utf8_lossy(&request[..length]).into_owned())
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}")
                .unwrap();
        });
        let mut http = ProviderHttp::new();

        let value: serde_json::Value = http
            .get_json(
                &format!("http://{address}/test"),
                "test request",
                Duration::ZERO,
                Instant::now() + Duration::from_secs(2),
                &mut (),
            )
            .unwrap();
        server.join().unwrap();
        let request = received.recv().unwrap();

        assert_eq!(value["ok"], true);
        assert!(request.to_ascii_lowercase().contains(&format!(
            "user-agent: music-groomer/{} (https://github.com/cark/music-groomer)",
            env!("CARGO_PKG_VERSION")
        )));
    }

    #[test]
    fn retry_after_is_respected_with_a_reasonable_cap() {
        assert_eq!(retry_delay(Some(7), 2), 7);
        assert_eq!(retry_delay(Some(600), 2), 15);
        assert_eq!(retry_delay(None, 4), 4);
        assert!(transient_status(429));
        assert!(transient_status(503));
        assert!(!transient_status(404));
    }
}
