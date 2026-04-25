//! Cloud fallback for `<|fault|>{"needs_cloud":true,…}`.
//!
//! Targets the OpenAI-compatible `/v1/chat/completions` shape. The same
//! adapter works against `claude.ai`, Kimi K2.5, vLLM, etc. — drop a base
//! URL + API key into env vars and the daemon doesn't care which provider.
//!
//! Env vars:
//! - `LLM_OS_CLOUD_URL` — base URL, default `https://api.openai.com/v1`.
//! - `LLM_OS_CLOUD_KEY` — bearer token; if unset, no auth header sent (useful
//!   for self-hosted vLLM with auth disabled).
//! - `LLM_OS_CLOUD_MODEL` — model id, default `gpt-4o-mini`.
//!
//! v0.01 sends a single non-streaming request and returns the assistant
//! message content as a string. v0.1 streams and supports chained tool use.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum CloudError {
    #[error("HTTP error: {0}")]
    Http(#[from] ureq::Error),
    #[error("response missing choices[0].message.content")]
    EmptyResponse,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json parse failed: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize)]
struct Request {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

pub struct CloudConfig {
    pub url: String,
    pub key: Option<String>,
    pub model: String,
    pub timeout: Duration,
}

impl CloudConfig {
    pub fn from_env() -> Self {
        Self {
            url: std::env::var("LLM_OS_CLOUD_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            key: std::env::var("LLM_OS_CLOUD_KEY").ok(),
            model: std::env::var("LLM_OS_CLOUD_MODEL")
                .unwrap_or_else(|_| "gpt-4o-mini".into()),
            timeout: Duration::from_secs(30),
        }
    }
}

/// Forward a fault payload to the cloud and return the assistant's reply.
///
/// `fault_payload` is whatever the model emitted as the JSON arg of
/// `<|fault|>` — we wrap it as the user message and let the cloud model
/// decide what to do. The local context (last N messages) is included to
/// give the cloud model enough state to be useful.
pub fn forward_fault(
    cfg: &CloudConfig,
    local_context: &[Message],
    fault_payload: &Value,
) -> Result<String, CloudError> {
    let mut messages = vec![Message {
        role: "system".into(),
        content: "You are the cloud fallback for an LLM-OS local kernel. \
                  The local model raised a fault. Read the fault payload \
                  and produce a JSON value the local model can inject as \
                  the result of the faulting opcode."
            .into(),
    }];
    messages.extend_from_slice(local_context);
    messages.push(Message {
        role: "user".into(),
        content: format!(
            "Local kernel fault payload (raw JSON):\n{}\n\nReturn a single JSON value the local model should consume.",
            serde_json::to_string_pretty(fault_payload)?
        ),
    });

    let req = Request {
        model: cfg.model.clone(),
        messages,
        temperature: Some(0.2),
        max_tokens: Some(512),
    };

    let url = format!("{}/chat/completions", cfg.url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new()
        .timeout(cfg.timeout)
        .build();
    let mut request = agent.post(&url).set("Content-Type", "application/json");
    if let Some(key) = &cfg.key {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    let body = serde_json::to_string(&req)?;
    let resp: Value = request.send_string(&body)?.into_json()?;

    let content = resp
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .ok_or(CloudError::EmptyResponse)?;

    Ok(content.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_from_env() {
        // Test invariant only — actual env may or may not be set.
        let cfg = CloudConfig::from_env();
        assert!(!cfg.url.is_empty());
        assert!(!cfg.model.is_empty());
        assert!(cfg.timeout >= Duration::from_secs(1));
    }
}
