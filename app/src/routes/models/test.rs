// Model availability test: POST /api/models/test.
//
// Sends a minimal completion request ("Reply with OK only.", max output 16
// tokens, no retries, 20s timeout) to the provider's protocol endpoint and
// reports ok/latency/HTTP status/response text. Mirrors pi-web's test route
// (research/pi-web §3, design §5). Pure helpers are unit-tested here; live
// HTTP is exercised by the functional test later.

use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::state::AppState;
use super::discover::build_headers;
use super::store::{read_config, StoreError};

/// Timeout for the minimal completion request (design §5: 20s).
const TEST_TIMEOUT: Duration = Duration::from_secs(20);
/// Max output tokens for the probe (pi-web §3).
const MAX_OUTPUT_TOKENS: u64 = 16;
/// The exact probe prompt (pi-web §3).
const PROBE_PROMPT: &str = "Reply with OK only.";
/// Truncation length for the response text snippet (pi-web §3: 300 chars).
const RESPONSE_TEXT_MAX: usize = 300;

// ── request / response types ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TestRequest {
    pub providerId: String,
    pub modelId: String,
    /// Override the provider's stored protocol (e.g. the claude tab will pass
    /// "anthropic-messages"). Defaults to provider.api when absent.
    #[serde(default)]
    pub protocol: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TestResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_text: Option<String>,
}

// ── handler ───────────────────────────────────────────────────────

pub async fn test(
    State(state): State<AppState>,
    Json(req): Json<TestRequest>,
) -> Result<Json<TestResponse>, (StatusCode, String)> {
    if req.providerId.trim().is_empty() || req.modelId.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "providerId and modelId are required".to_string(),
        ));
    }

    // Resolve the provider (real key, no mask semantics here).
    let config = match read_config(&state.models_file) {
        Ok(c) => c,
        Err(StoreError::Io(e)) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read models.json: {e}"),
            ));
        }
        Err(StoreError::Corrupt(e)) => {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                format!("models.json corrupt: {e}"),
            ));
        }
    };
    let provider = config.providers.get(&req.providerId).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("provider '{}' not found", req.providerId),
        )
    })?;

    let protocol = req
        .protocol
        .clone()
        .unwrap_or_else(|| provider.api.clone());

    // R1: the provider's baseUrl IS the endpoint for every protocol —
    // protocol selection decides the request shape, so test against it
    // directly (no separate anthropic override block).
    let base_url = provider.base_url.clone();

    // No key => error, but still HTTP 200 with ok:false (UI decides).
    let key = provider.api_key.clone();
    if key.as_deref().map_or(true, |k| k.is_empty()) {
        return Ok(Json(TestResponse {
            ok: false,
            latency_ms: None,
            status: None,
            error: Some(format!(
                "No API key found for \"{}\"",
                req.providerId
            )),
            response_text: None,
        }));
    }

    let headers = build_headers(&protocol, key.as_deref(), &provider.headers);
    let endpoint = completion_url(&base_url, &protocol);
    let body = completion_body(&req.modelId, &protocol);

    let start = Instant::now();
    let mut req_builder = state
        .http
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .timeout(TEST_TIMEOUT);
    for (k, v) in &headers {
        req_builder = req_builder.header(k, v);
    }
    let resp = req_builder.json(&body).send().await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match resp {
        Err(e) => {
            let msg = if e.is_timeout() {
                format!("timeout after {}s", TEST_TIMEOUT.as_secs())
            } else {
                // Connect/transport error string (e.g. "dns error", "connection
                // refused"). reqwest's Display is human-readable enough.
                e.to_string()
            };
            Ok(Json(TestResponse {
                ok: false,
                latency_ms: Some(latency_ms),
                status: None,
                error: Some(truncate(&msg, RESPONSE_TEXT_MAX)),
                response_text: None,
            }))
        }
        Ok(r) => {
            let status = r.status();
            let status_code = status.as_u16();
            let text = r.text().await.unwrap_or_default();
            if status.is_success() {
                let response_text = extract_response_text(&text, &protocol);
                Ok(Json(TestResponse {
                    ok: true,
                    latency_ms: Some(latency_ms),
                    status: Some(status_code),
                    error: None,
                    response_text: Some(truncate(&response_text, RESPONSE_TEXT_MAX)),
                }))
            } else {
                Ok(Json(TestResponse {
                    ok: false,
                    latency_ms: Some(latency_ms),
                    status: Some(status_code),
                    error: Some(truncate(&text, RESPONSE_TEXT_MAX)),
                    response_text: None,
                }))
            }
        }
    }
}

// ── URL derivation (pure) ─────────────────────────────────────────

/// Derive the completion endpoint URL for a protocol (design §5 / handler
/// spec).
///
/// - openai-completions -> `<base>/chat/completions`
/// - openai-responses  -> `<base>/responses`
/// - anthropic-messages -> `<base>/v1/messages`
pub fn completion_url(base: &str, protocol: &str) -> String {
    let base = base.trim_end_matches('/');
    match protocol {
        "openai-completions" => format!("{base}/chat/completions"),
        "openai-responses" => format!("{base}/responses"),
        "anthropic-messages" => format!("{base}/v1/messages"),
        _ => format!("{base}/chat/completions"),
    }
}

// ── request body construction (pure) ───────────────────────────────

/// Build the minimal completion request body for a protocol (design §5).
pub fn completion_body(model: &str, protocol: &str) -> Value {
    match protocol {
        "openai-completions" => json!({
            "model": model,
            "messages": [{"role":"user","content": PROBE_PROMPT}],
            "max_tokens": MAX_OUTPUT_TOKENS,
            "stream": false,
        }),
        "openai-responses" => json!({
            "model": model,
            "input": PROBE_PROMPT,
            "max_output_tokens": MAX_OUTPUT_TOKENS,
        }),
        "anthropic-messages" => json!({
            "model": model,
            "max_tokens": MAX_OUTPUT_TOKENS,
            "messages": [{"role":"user","content": PROBE_PROMPT}],
        }),
        _ => json!({
            "model": model,
            "messages": [{"role":"user","content": PROBE_PROMPT}],
            "max_tokens": MAX_OUTPUT_TOKENS,
            "stream": false,
        }),
    }
}

// ── response extraction (pure) ────────────────────────────────────

/// Extract the first text content from a completion response, leniently
/// (never fails — returns "" on any extraction problem, design §5).
pub fn extract_response_text(body: &str, protocol: &str) -> String {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    match protocol {
        "openai-completions" => v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
        "openai-responses" => {
            // OpenAI Responses API: top-level `output_text`, or
            // output[].content[].text.
            if let Some(s) = v.get("output_text").and_then(|t| t.as_str()) {
                return s.to_string();
            }
            v.get("output")
                .and_then(|o| o.as_array())
                .and_then(|arr| {
                    arr.iter().find_map(|item| {
                        item.get("content")
                            .and_then(|c| c.as_array())
                            .and_then(|c| {
                                c.iter().find_map(|ci| {
                                    if ci.get("type").and_then(|t| t.as_str()) == Some("text") {
                                        ci.get("text").and_then(|t| t.as_str()).map(String::from)
                                    } else {
                                        None
                                    }
                                })
                            })
                    })
                })
                .unwrap_or_default()
        }
        "anthropic-messages" => v
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| {
                arr.iter().find_map(|ci| {
                    if ci.get("type").and_then(|t| t.as_str()) == Some("text") {
                        ci.get("text").and_then(|t| t.as_str()).map(String::from)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_default(),
        _ => v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default(),
    }
}

/// Truncate to `max` chars (char count, not bytes), appending "…" when
/// truncated.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max).collect();
        format!("{head}…")
    }
}

// ── tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- completion_url ---

    #[test]
    fn url_openai_completions() {
        assert_eq!(
            completion_url("https://api.x.com/v1", "openai-completions"),
            "https://api.x.com/v1/chat/completions"
        );
    }

    #[test]
    fn url_openai_responses() {
        assert_eq!(
            completion_url("https://api.x.com/v1", "openai-responses"),
            "https://api.x.com/v1/responses"
        );
    }

    #[test]
    fn url_anthropic_messages() {
        assert_eq!(
            completion_url("https://api.anthropic.com", "anthropic-messages"),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn url_strips_trailing_slash() {
        assert_eq!(
            completion_url("https://api.x.com/v1/", "openai-completions"),
            "https://api.x.com/v1/chat/completions"
        );
    }

    // --- completion_body ---

    #[test]
    fn body_openai_completions_has_prompt_and_max_tokens() {
        let b = completion_body("gpt-4", "openai-completions");
        assert_eq!(b["model"], "gpt-4");
        assert_eq!(b["max_tokens"], 16);
        assert_eq!(b["stream"], false);
        assert_eq!(
            b["messages"][0]["content"],
            "Reply with OK only."
        );
    }

    #[test]
    fn body_openai_responses_uses_input_field() {
        let b = completion_body("gpt-4", "openai-responses");
        assert_eq!(b["model"], "gpt-4");
        assert_eq!(b["max_output_tokens"], 16);
        assert_eq!(b["input"], "Reply with OK only.");
        assert!(b.get("messages").is_none());
    }

    #[test]
    fn body_anthropic_messages_shape() {
        let b = completion_body("claude-3", "anthropic-messages");
        assert_eq!(b["model"], "claude-3");
        assert_eq!(b["max_tokens"], 16);
        assert_eq!(
            b["messages"][0]["content"],
            "Reply with OK only."
        );
        // anthropic body has no stream field.
        assert!(b.get("stream").is_none());
    }

    // --- extract_response_text ---

    #[test]
    fn extract_openai_chat_text() {
        let body = r#"{"choices":[{"message":{"content":"OK"}}]}"#;
        assert_eq!(
            extract_response_text(body, "openai-completions"),
            "OK"
        );
    }

    #[test]
    fn extract_openai_responses_output_text() {
        let body = r#"{"output_text":"OK"}"#;
        assert_eq!(
            extract_response_text(body, "openai-responses"),
            "OK"
        );
    }

    #[test]
    fn extract_openai_responses_nested_content() {
        let body = r#"{"output":[{"content":[{"type":"text","text":"OK"}]}]}"#;
        assert_eq!(
            extract_response_text(body, "openai-responses"),
            "OK"
        );
    }

    #[test]
    fn extract_anthropic_text() {
        let body = r#"{"content":[{"type":"text","text":"OK"}]}"#;
        assert_eq!(
            extract_response_text(body, "anthropic-messages"),
            "OK"
        );
    }

    #[test]
    fn extract_skips_non_text_content_blocks() {
        let body =
            r#"{"content":[{"type":"thinking","text":"hidden"},{"type":"text","text":"OK"}]}"#;
        assert_eq!(
            extract_response_text(body, "anthropic-messages"),
            "OK"
        );
    }

    #[test]
    fn extract_invalid_json_returns_empty() {
        assert_eq!(
            extract_response_text("not json", "openai-completions"),
            ""
        );
    }

    #[test]
    fn extract_missing_choices_returns_empty() {
        assert_eq!(
            extract_response_text("{}", "openai-completions"),
            ""
        );
    }

    // --- truncate ---

    #[test]
    fn truncate_short_unchanged() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_long_gets_ellipsis() {
        let s = "a".repeat(400);
        let t = truncate(&s, RESPONSE_TEXT_MAX);
        assert!(t.ends_with('…'));
        assert_eq!(t.chars().count(), RESPONSE_TEXT_MAX + 1);
    }
}
