#![cfg(target_os = "macos")]

use vox::chat::{self, Message};

#[test]
fn build_claude_request_basic() {
    let messages = vec![Message {
        role: "user".to_string(),
        content: "Bonjour".to_string(),
    }];
    let req = chat::build_claude_request("claude-sonnet-4-20250514", &messages);
    assert_eq!(req.model, "claude-sonnet-4-20250514");
    assert_eq!(req.max_tokens, 1024);
    assert!(!req.system.is_empty());
    assert_eq!(req.messages.len(), 1);
    assert_eq!(req.messages[0].role, "user");
    assert_eq!(req.messages[0].content, "Bonjour");
}

#[test]
fn build_claude_request_preserves_history() {
    let messages = vec![
        Message {
            role: "user".to_string(),
            content: "Salut".to_string(),
        },
        Message {
            role: "assistant".to_string(),
            content: "Bonjour !".to_string(),
        },
        Message {
            role: "user".to_string(),
            content: "Comment vas-tu ?".to_string(),
        },
    ];
    let req = chat::build_claude_request("claude-sonnet-4-20250514", &messages);
    assert_eq!(req.messages.len(), 3);
    assert_eq!(req.messages[0].role, "user");
    assert_eq!(req.messages[1].role, "assistant");
    assert_eq!(req.messages[2].role, "user");
}

#[test]
fn build_claude_request_custom_model() {
    let messages = vec![Message {
        role: "user".to_string(),
        content: "Test".to_string(),
    }];
    let req = chat::build_claude_request("claude-opus-4-20250514", &messages);
    assert_eq!(req.model, "claude-opus-4-20250514");
}

#[test]
fn parse_claude_response_valid() {
    let body = r#"{"id":"msg_123","type":"message","role":"assistant","content":[{"type":"text","text":"Bonjour !"}],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":5}}"#;
    let text = chat::parse_claude_response(body).unwrap();
    assert_eq!(text, "Bonjour !");
}

#[test]
fn parse_claude_response_empty_content() {
    let body = r#"{"id":"msg_123","type":"message","role":"assistant","content":[],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":0}}"#;
    let result = chat::parse_claude_response(body);
    assert!(result.is_err());
}

#[test]
fn parse_claude_response_invalid_json() {
    let result = chat::parse_claude_response("not json");
    assert!(result.is_err());
}

#[test]
fn parse_claude_response_multiblock() {
    let body = r#"{"id":"msg_123","type":"message","role":"assistant","content":[{"type":"text","text":"Premier"},{"type":"text","text":"Deuxième"}],"model":"claude-sonnet-4-20250514","stop_reason":"end_turn","usage":{"input_tokens":10,"output_tokens":10}}"#;
    let text = chat::parse_claude_response(body).unwrap();
    // Should return first block
    assert_eq!(text, "Premier");
}
