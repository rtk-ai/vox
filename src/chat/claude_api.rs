use std::sync::mpsc;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

use super::{API_URL, API_VERSION, MAX_TOKENS, Message, SYSTEM_PROMPT};

#[derive(Debug)]
pub enum StreamEvent {
    TextDelta(String),
    Done,
}

#[derive(Serialize)]
struct StreamRequest {
    model: String,
    max_tokens: u32,
    system: String,
    messages: Vec<Message>,
    stream: bool,
}

#[derive(Deserialize)]
struct SseData {
    #[serde(rename = "type")]
    type_field: String,
    delta: Option<SseDelta>,
}

#[derive(Deserialize)]
struct SseDelta {
    #[serde(rename = "type")]
    type_field: Option<String>,
    text: Option<String>,
}

#[derive(Serialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub max_tokens: u32,
    pub system: String,
    pub messages: Vec<Message>,
}

#[derive(Deserialize)]
pub struct ClaudeResponse {
    pub content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
pub struct ContentBlock {
    pub text: String,
}

pub fn build_claude_request(model: &str, messages: &[Message]) -> ClaudeRequest {
    ClaudeRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: SYSTEM_PROMPT.to_string(),
        messages: messages.to_vec(),
    }
}

pub fn parse_claude_response(body: &str) -> Result<String> {
    let resp: ClaudeResponse =
        serde_json::from_str(body).context("Failed to parse Claude API response")?;
    resp.content
        .first()
        .map(|b| b.text.clone())
        .ok_or_else(|| anyhow::anyhow!("Empty response from Claude"))
}

/// Blocking Claude API call (kept for compatibility).
pub fn call_claude(api_key: &str, model: &str, messages: &[Message]) -> Result<String> {
    let request = build_claude_request(model, messages);

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .context("Failed to call Claude API")?;

    let status = resp.status();
    let body = resp.text().context("Failed to read Claude API response")?;
    if !status.is_success() {
        anyhow::bail!("Claude API error ({status}): {body}");
    }

    parse_claude_response(&body)
}

/// Stream Claude API response via SSE, sending text deltas through the channel.
pub fn stream_claude(
    api_key: &str,
    model: &str,
    messages: &[Message],
    tx: mpsc::Sender<StreamEvent>,
) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime")?;
    rt.block_on(stream_claude_inner(api_key, model, messages, tx))
}

async fn stream_claude_inner(
    api_key: &str,
    model: &str,
    messages: &[Message],
    tx: mpsc::Sender<StreamEvent>,
) -> Result<()> {
    let request = StreamRequest {
        model: model.to_string(),
        max_tokens: MAX_TOKENS,
        system: SYSTEM_PROMPT.to_string(),
        messages: messages.to_vec(),
        stream: true,
    };

    let client = reqwest::Client::new();
    let resp = client
        .post(API_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .context("Failed to call Claude API")?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Claude API error: {body}");
    }

    let mut stream = resp.bytes_stream();
    let mut line_buf = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.context("stream read error")?;
        line_buf.push_str(&String::from_utf8_lossy(&bytes));

        // Process complete lines from the SSE stream
        while let Some(pos) = line_buf.find('\n') {
            let line = line_buf[..pos].trim_end_matches('\r').to_string();
            line_buf = line_buf[pos + 1..].to_string();

            let Some(data) = line.strip_prefix("data: ") else {
                continue;
            };

            if data == "[DONE]" {
                let _ = tx.send(StreamEvent::Done);
                return Ok(());
            }

            let Ok(event) = serde_json::from_str::<SseData>(data) else {
                continue;
            };

            match event.type_field.as_str() {
                "content_block_delta" => {
                    if let Some(delta) = event.delta
                        && delta.type_field.as_deref() == Some("text_delta")
                        && let Some(text) = delta.text
                    {
                        let _ = tx.send(StreamEvent::TextDelta(text));
                    }
                }
                "message_stop" => {
                    let _ = tx.send(StreamEvent::Done);
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    // Stream ended without explicit message_stop
    let _ = tx.send(StreamEvent::Done);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_parsing() {
        // Simulate SSE content_block_delta
        let data = r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Bonjour"}}"#;
        let event: SseData = serde_json::from_str(data).unwrap();
        assert_eq!(event.type_field, "content_block_delta");
        let delta = event.delta.unwrap();
        assert_eq!(delta.type_field.as_deref(), Some("text_delta"));
        assert_eq!(delta.text.as_deref(), Some("Bonjour"));
    }

    #[test]
    fn test_sse_message_stop() {
        let data = r#"{"type":"message_stop"}"#;
        let event: SseData = serde_json::from_str(data).unwrap();
        assert_eq!(event.type_field, "message_stop");
        assert!(event.delta.is_none());
    }
}
