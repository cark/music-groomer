use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::http::ProviderHttp;
use super::{ProviderError, ProviderName, ProviderProgress};
use crate::fingerprint::AudioFingerprint;

const API_URL: &str = "https://api.acoustid.org/v2/lookup";
const APPLICATION_KEY: &str = "5gtrNQDixK";
const FAILURE_RETRY_BUDGET: Duration = Duration::from_secs(30);
pub const ACOUSTID_USABLE_SCORE: f64 = 0.80;

pub trait AcoustIdProvider {
    fn lookup(
        &mut self,
        fingerprint: &AudioFingerprint,
        progress: &mut dyn ProviderProgress,
    ) -> Result<AcoustIdResponse, ProviderError>;
}

impl<T: AcoustIdProvider + ?Sized> AcoustIdProvider for &mut T {
    fn lookup(
        &mut self,
        fingerprint: &AudioFingerprint,
        progress: &mut dyn ProviderProgress,
    ) -> Result<AcoustIdResponse, ProviderError> {
        (**self).lookup(fingerprint, progress)
    }
}

pub struct AcoustId {
    client: AcoustIdClient<ProviderHttp>,
}

impl AcoustId {
    pub fn new() -> Self {
        Self {
            client: AcoustIdClient {
                http: ProviderHttp::with_failure_budget(
                    ProviderName::AcoustId,
                    FAILURE_RETRY_BUDGET,
                ),
            },
        }
    }
}

impl Default for AcoustId {
    fn default() -> Self {
        Self::new()
    }
}

impl AcoustIdProvider for AcoustId {
    fn lookup(
        &mut self,
        fingerprint: &AudioFingerprint,
        progress: &mut dyn ProviderProgress,
    ) -> Result<AcoustIdResponse, ProviderError> {
        self.client.lookup(fingerprint, progress)
    }
}

trait AcoustIdHttp {
    fn post_json<T: serde::de::DeserializeOwned>(
        &mut self,
        form: &str,
        progress: &mut dyn ProviderProgress,
    ) -> Result<T, ProviderError>;
}

impl AcoustIdHttp for ProviderHttp {
    fn post_json<T: serde::de::DeserializeOwned>(
        &mut self,
        form: &str,
        progress: &mut dyn ProviderProgress,
    ) -> Result<T, ProviderError> {
        self.post_form_json(API_URL, "AcoustID fingerprint lookup", form, progress)
    }
}

struct AcoustIdClient<H> {
    http: H,
}

impl<H: AcoustIdHttp> AcoustIdClient<H> {
    fn lookup(
        &mut self,
        fingerprint: &AudioFingerprint,
        progress: &mut dyn ProviderProgress,
    ) -> Result<AcoustIdResponse, ProviderError> {
        let form = lookup_form(fingerprint);
        let response: WireResponse = self.http.post_json(&form, progress)?;
        if response.status != "ok" {
            let detail = response
                .error
                .and_then(|error| {
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                })
                .unwrap_or(response.status);
            return Err(ProviderError::InvalidResponse(format!(
                "AcoustID rejected the lookup: {detail}"
            )));
        }
        Ok(AcoustIdResponse {
            results: response
                .results
                .into_iter()
                .map(|result| AcoustIdResult {
                    id: result.id,
                    score: result.score,
                    recording_ids: result
                        .recordings
                        .into_iter()
                        .map(|recording| recording.id)
                        .collect(),
                })
                .collect(),
        })
    }
}

fn lookup_form(fingerprint: &AudioFingerprint) -> String {
    [
        ("client", APPLICATION_KEY.to_owned()),
        ("clientversion", env!("CARGO_PKG_VERSION").to_owned()),
        ("format", "json".to_owned()),
        ("meta", "recordingids".to_owned()),
        ("duration", fingerprint.duration_seconds.to_string()),
        ("fingerprint", fingerprint.value.clone()),
    ]
    .into_iter()
    .map(|(name, value)| format!("{name}={}", urlencoding::encode(&value)))
    .collect::<Vec<_>>()
    .join("&")
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AcoustIdResponse {
    pub results: Vec<AcoustIdResult>,
}

impl AcoustIdResponse {
    pub fn has_usable_recording_associations(&self) -> bool {
        self.results
            .iter()
            .any(|result| result.score >= ACOUSTID_USABLE_SCORE && !result.recording_ids.is_empty())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AcoustIdResult {
    pub id: String,
    pub score: f64,
    pub recording_ids: Vec<String>,
}

#[derive(Deserialize)]
struct WireResponse {
    status: String,
    #[serde(default)]
    results: Vec<WireResult>,
    error: Option<Value>,
}

#[derive(Deserialize)]
struct WireResult {
    id: String,
    score: f64,
    #[serde(default)]
    recordings: Vec<WireRecording>,
}

#[derive(Deserialize)]
struct WireRecording {
    id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHttp {
        form: String,
        response: Value,
    }

    impl AcoustIdHttp for FakeHttp {
        fn post_json<T: serde::de::DeserializeOwned>(
            &mut self,
            form: &str,
            _progress: &mut dyn ProviderProgress,
        ) -> Result<T, ProviderError> {
            self.form = form.to_owned();
            serde_json::from_value(self.response.clone())
                .map_err(|error| ProviderError::InvalidResponse(error.to_string()))
        }
    }

    #[test]
    fn sends_only_lookup_identity_and_maps_recording_associations() {
        let mut client = AcoustIdClient {
            http: FakeHttp {
                form: String::new(),
                response: serde_json::json!({
                    "status": "ok",
                    "results": [{
                        "id": "acoustid-result",
                        "score": 0.97,
                        "recordings": [{"id": "recording-id"}]
                    }]
                }),
            },
        };

        let result = client
            .lookup(
                &AudioFingerprint {
                    duration_seconds: 181,
                    value: "A+/long fingerprint".into(),
                },
                &mut (),
            )
            .unwrap();

        assert_eq!(result.results[0].recording_ids, ["recording-id"]);
        assert!(client.http.form.contains("client=5gtrNQDixK"));
        assert!(client.http.form.contains("clientversion=0.1.0"));
        assert!(client.http.form.contains("meta=recordingids"));
        assert!(
            client
                .http
                .form
                .contains("fingerprint=A%2B%2Flong%20fingerprint")
        );
        assert!(!client.http.form.contains("release"));
    }
}
