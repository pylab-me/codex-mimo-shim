use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::error::ShimError;
use crate::responses::tools::{
    ToolIdentityRegistry, make_assistant_tool_calls_message, make_tool_message,
    response_function_call_item_to_chat_tool_call, responses_tools_to_chat_tools,
};
use crate::xiaomi::reasoning::{
    ReasoningPolicy, apply_reasoning_policy_to_chat_messages, copy_incoming_reasoning_content,
    insert_thinking_option,
};

#[derive(Debug, Clone, Default)]
pub struct ConversionDefaults {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct ConvertedRequest {
    pub response_id: String,
    pub stream: bool,
    pub store: bool,
    pub previous_response_id: Option<String>,
    pub client_model: String,
    pub provider_model: String,
    pub chat_payload: Value,
    pub chat_messages: Vec<Value>,
    pub parallel_tool_calls: bool,
    pub tool_registry: ToolIdentityRegistry,
}

pub fn convert_responses_to_chat(
    request: &Value,
    previous_messages: Option<Vec<Value>>,
    client_model: &str,
    provider_model: &str,
    forward_parallel_tool_calls: bool,
    defaults: &ConversionDefaults,
    reasoning_policy: ReasoningPolicy,
) -> Result<ConvertedRequest, ShimError> {
    let obj = request.as_object().ok_or_else(|| {
        ShimError::InvalidRequest("request body must be a JSON object".to_string())
    })?;
    let client_model = client_model.to_string();
    let provider_model = provider_model.to_string();
    let stream = obj.get("stream").and_then(Value::as_bool).unwrap_or(false);
    let store = obj.get("store").and_then(Value::as_bool).unwrap_or(true);
    let previous_response_id = obj
        .get("previous_response_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);

    let mut messages = previous_messages.unwrap_or_default();
    if messages.is_empty() {
        if let Some(instructions) = obj
            .get("instructions")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        {
            messages.push(json!({"role":"system", "content": instructions}));
        }
    }

    let input = obj.get("input").cloned().unwrap_or_else(|| json!(""));
    messages.extend(input_to_chat_messages(&input, reasoning_policy)?);
    apply_reasoning_policy_to_chat_messages(&mut messages, reasoning_policy);

    let mut payload = Map::new();
    payload.insert("model".to_string(), json!(provider_model));
    payload.insert("messages".to_string(), Value::Array(messages.clone()));
    payload.insert("stream".to_string(), json!(false)); // buffered SSE externally, non-streaming upstream.
    insert_thinking_option(&mut payload, reasoning_policy);

    copy_number_or_default(obj, &mut payload, "temperature", defaults.temperature);
    copy_number_or_default(obj, &mut payload, "top_p", defaults.top_p);
    if let Some(value) = obj.get("max_output_tokens") {
        if value.is_number() {
            payload.insert("max_tokens".to_string(), value.clone());
        }
    } else if let Some(value) = defaults.max_output_tokens {
        payload.insert("max_tokens".to_string(), json!(value));
    }

    let tool_conversion = responses_tools_to_chat_tools(obj.get("tools"));
    if !tool_conversion.tools.is_empty() {
        payload.insert("tools".to_string(), Value::Array(tool_conversion.tools));
        if let Some(tool_choice) = obj.get("tool_choice") {
            payload.insert("tool_choice".to_string(), tool_choice.clone());
        }
        if forward_parallel_tool_calls {
            if let Some(parallel) = obj.get("parallel_tool_calls") {
                payload.insert("parallel_tool_calls".to_string(), parallel.clone());
            }
        }
    }

    let parallel_tool_calls = obj
        .get("parallel_tool_calls")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && forward_parallel_tool_calls;

    Ok(ConvertedRequest {
        response_id: format!("resp_local_{}", Uuid::new_v4().simple()),
        stream,
        store,
        previous_response_id,
        client_model,
        provider_model: payload
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("mimo-v2.5")
            .to_string(),
        chat_payload: Value::Object(payload),
        chat_messages: messages,
        parallel_tool_calls,
        tool_registry: tool_conversion.registry,
    })
}

fn copy_number_or_default(
    src: &Map<String, Value>,
    dst: &mut Map<String, Value>,
    key: &str,
    default: Option<f64>,
) {
    if let Some(value) = src.get(key) {
        if value.is_number() {
            dst.insert(key.to_string(), value.clone());
        }
    } else if let Some(value) = default {
        dst.insert(key.to_string(), json!(value));
    }
}

fn input_to_chat_messages(
    input: &Value,
    reasoning_policy: ReasoningPolicy,
) -> Result<Vec<Value>, ShimError> {
    match input {
        Value::String(text) => Ok(vec![json!({"role":"user", "content": text})]),
        Value::Array(items) => input_items_to_chat_messages(items, reasoning_policy),
        _ => Err(ShimError::InvalidRequest(
            "input must be string or array".to_string(),
        )),
    }
}

fn input_items_to_chat_messages(
    items: &[Value],
    reasoning_policy: ReasoningPolicy,
) -> Result<Vec<Value>, ShimError> {
    let mut messages = Vec::new();
    let mut pending_assistant_tool_calls = Vec::new();

    for item in items {
        let Some(obj) = item.as_object() else {
            continue;
        };

        match obj.get("type").and_then(Value::as_str) {
            Some("function_call") | Some("custom_tool_call") => {
                pending_assistant_tool_calls
                    .push(response_function_call_item_to_chat_tool_call(item));
            }
            Some("function_call_output") | Some("custom_tool_call_output") => {
                flush_pending_tool_calls(&mut messages, &mut pending_assistant_tool_calls);
                let call_id = obj
                    .get("call_id")
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| {
                        obj.get("id")
                            .and_then(Value::as_str)
                            .unwrap_or("call_local_unknown")
                    });
                let output = obj.get("output").cloned().unwrap_or(Value::Null);
                messages.push(make_tool_message(call_id, &output));
            }
            Some("message") | None => {
                flush_pending_tool_calls(&mut messages, &mut pending_assistant_tool_calls);
                let role = obj.get("role").and_then(Value::as_str).unwrap_or("user");
                let role = if role == "developer" { "system" } else { role };
                let content = normalize_content(obj.get("content"))?;
                let mut message = Map::new();
                message.insert("role".to_string(), json!(role));
                message.insert("content".to_string(), content);
                copy_incoming_reasoning_content(obj, &mut message, role, reasoning_policy);
                messages.push(Value::Object(message));
            }
            // Unknown Responses item types are ignored. This is a permissive Codex shim,
            // not a full Responses validator.
            _ => {}
        }
    }

    flush_pending_tool_calls(&mut messages, &mut pending_assistant_tool_calls);
    Ok(messages)
}

fn flush_pending_tool_calls(messages: &mut Vec<Value>, pending: &mut Vec<Value>) {
    if !pending.is_empty() {
        messages.push(make_assistant_tool_calls_message(std::mem::take(pending)));
    }
}

const MAX_BASE64_STRING_BYTES: usize = 50 * 1024 * 1024;

fn normalize_content(content: Option<&Value>) -> Result<Value, ShimError> {
    match content {
        Some(Value::String(text)) => Ok(json!(text)),
        Some(Value::Array(parts)) => {
            let mut normalized_parts = Vec::new();
            for part in parts {
                if let Some(normalized) = normalize_content_part(part)? {
                    normalized_parts.push(normalized);
                }
            }
            Ok(Value::Array(normalized_parts))
        }
        Some(other) => Ok(json!(other.to_string())),
        None => Ok(json!("")),
    }
}

fn normalize_content_part(part: &Value) -> Result<Option<Value>, ShimError> {
    let Some(obj) = part.as_object() else {
        return Ok(None);
    };

    match obj.get("type").and_then(Value::as_str) {
        Some("text") | Some("input_text") | Some("output_text") => {
            let text = obj.get("text").and_then(Value::as_str).unwrap_or("");
            Ok(Some(json!({"type": "text", "text": text})))
        }
        Some("image_url") => normalize_image_url_part(obj),
        Some("input_image") => normalize_input_image_part(obj),
        Some("image") => normalize_image_part(obj),
        Some("input_audio") => normalize_input_audio_part(obj),
        Some(other) => Err(ShimError::InvalidRequest(format!(
            "unsupported content part type: {other}"
        ))),
        None => Ok(None),
    }
}

fn normalize_image_url_part(obj: &Map<String, Value>) -> Result<Option<Value>, ShimError> {
    if let Some(image_url) = obj.get("image_url") {
        let normalized = normalize_image_url_payload(image_url)?;
        return Ok(Some(json!({"type": "image_url", "image_url": normalized})));
    }

    if let Some(url) = obj.get("url").and_then(Value::as_str) {
        validate_image_data_url_if_present(url)?;
        return Ok(Some(
            json!({"type": "image_url", "image_url": {"url": url}}),
        ));
    }

    Err(ShimError::InvalidRequest(
        "image_url content part must contain image_url or url".to_string(),
    ))
}

fn normalize_input_image_part(obj: &Map<String, Value>) -> Result<Option<Value>, ShimError> {
    if let Some(image_url) = obj.get("image_url") {
        let normalized = normalize_image_url_payload(image_url)?;
        return Ok(Some(json!({"type": "image_url", "image_url": normalized})));
    }

    let url = obj.get("url").and_then(Value::as_str).ok_or_else(|| {
        ShimError::InvalidRequest(
            "input_image content part must contain image_url or url".to_string(),
        )
    })?;

    validate_image_data_url_if_present(url)?;
    Ok(Some(
        json!({"type": "image_url", "image_url": {"url": url}}),
    ))
}

fn normalize_image_part(obj: &Map<String, Value>) -> Result<Option<Value>, ShimError> {
    let source = obj.get("source").ok_or_else(|| {
        ShimError::InvalidRequest("image content part must contain source".to_string())
    })?;

    if let Some(url) = source.get("url").and_then(Value::as_str) {
        validate_image_data_url_if_present(url)?;
    }

    Ok(Some(json!({"type": "image", "source": source})))
}

fn normalize_input_audio_part(obj: &Map<String, Value>) -> Result<Option<Value>, ShimError> {
    let input_audio = obj
        .get("input_audio")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ShimError::InvalidRequest(
                "input_audio content part must contain input_audio object".to_string(),
            )
        })?;

    let data = input_audio
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ShimError::InvalidRequest(
                "input_audio.input_audio.data must be a base64 string or data URL".to_string(),
            )
        })?;

    let format_from_field = input_audio.get("format").and_then(Value::as_str);
    let inferred = validate_audio_data(data, format_from_field)?;

    let mut normalized_audio = input_audio.clone();
    if !normalized_audio.contains_key("format") {
        if let Some(format) = inferred {
            normalized_audio.insert("format".to_string(), json!(format));
        }
    }

    Ok(Some(
        json!({"type": "input_audio", "input_audio": normalized_audio}),
    ))
}

fn normalize_image_url_payload(image_url: &Value) -> Result<Value, ShimError> {
    if let Some(url) = image_url.get("url").and_then(Value::as_str) {
        validate_image_data_url_if_present(url)?;
        return Ok(image_url.clone());
    }
    if let Some(url) = image_url.as_str() {
        validate_image_data_url_if_present(url)?;
        return Ok(json!({"url": url}));
    }
    Err(ShimError::InvalidRequest(
        "image_url must be an object with url or a URL string".to_string(),
    ))
}

fn validate_image_data_url_if_present(url: &str) -> Result<(), ShimError> {
    if let Some((_mime, encoded)) = split_data_url(url)? {
        validate_base64_string_size(encoded, "image")?;
    }
    Ok(())
}

fn validate_audio_data(
    data: &str,
    explicit_format: Option<&str>,
) -> Result<Option<&'static str>, ShimError> {
    let mut inferred_format = None;

    if let Some((mime, encoded)) = split_data_url(data)? {
        validate_base64_string_size(encoded, "audio")?;
        inferred_format = Some(audio_format_from_mime(mime).ok_or_else(|| {
            ShimError::InvalidRequest(format!(
                "unsupported input_audio MIME type: {mime}; supported formats are mp3, wav, flac, m4a, ogg"
            ))
        })?);
    } else {
        validate_base64_string_size(data, "audio")?;
    }

    if let Some(format) = explicit_format {
        let normalized = normalize_audio_format(format).ok_or_else(|| {
            ShimError::InvalidRequest(format!(
                "unsupported input_audio format: {format}; supported formats are mp3, wav, flac, m4a, ogg"
            ))
        })?;
        if let Some(inferred) = inferred_format {
            if normalized != inferred {
                return Err(ShimError::InvalidRequest(format!(
                    "input_audio format mismatch: data URL MIME implies {inferred}, but format is {normalized}"
                )));
            }
        }
        return Ok(Some(normalized));
    }

    Ok(inferred_format)
}

fn split_data_url(value: &str) -> Result<Option<(&str, &str)>, ShimError> {
    if !value.starts_with("data:") {
        return Ok(None);
    }

    let Some((metadata, encoded)) = value.split_once(',') else {
        return Err(ShimError::InvalidRequest(
            "data URL must contain a comma before base64 payload".to_string(),
        ));
    };

    if !metadata.ends_with(";base64") {
        return Err(ShimError::InvalidRequest(
            "data URL must use ;base64 encoding".to_string(),
        ));
    }

    let mime = metadata
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .unwrap_or("");
    if mime.is_empty() {
        return Err(ShimError::InvalidRequest(
            "data URL must include a MIME type".to_string(),
        ));
    }

    Ok(Some((mime, encoded)))
}

fn validate_base64_string_size(encoded: &str, field_name: &str) -> Result<(), ShimError> {
    let len = encoded
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .count();
    if len > MAX_BASE64_STRING_BYTES {
        return Err(ShimError::InvalidRequest(format!(
            "{field_name} base64 string exceeds 50 MB limit"
        )));
    }
    Ok(())
}

fn audio_format_from_mime(mime: &str) -> Option<&'static str> {
    match mime.to_ascii_lowercase().as_str() {
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/wav" | "audio/wave" | "audio/x-wav" => Some("wav"),
        "audio/flac" => Some("flac"),
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" => Some("m4a"),
        "audio/ogg" | "application/ogg" => Some("ogg"),
        _ => None,
    }
}

fn normalize_audio_format(format: &str) -> Option<&'static str> {
    match format.to_ascii_lowercase().as_str() {
        "mp3" => Some("mp3"),
        "wav" | "wave" => Some("wav"),
        "flac" => Some("flac"),
        "m4a" | "mp4" => Some("m4a"),
        "ogg" | "oga" => Some("ogg"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_client_model_from_provider_model() {
        let request = json!({
            "model": "codex-auto-review",
            "input": "review this change"
        });

        let converted = convert_responses_to_chat(
            &request,
            None,
            "codex-auto-review",
            "mimo-v2.5",
            false,
            &ConversionDefaults::default(),
            ReasoningPolicy::default(),
        )
        .expect("conversion should succeed");

        assert_eq!(converted.client_model, "codex-auto-review");
        assert_eq!(converted.provider_model, "mimo-v2.5");
        assert_eq!(converted.chat_payload["model"], json!("mimo-v2.5"));
    }

    #[test]
    fn preserves_image_url_content_parts() {
        let request = json!({
            "model": "mimo-v2.5",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [
                    {"type":"image_url", "image_url":{"url":"data:image/png;base64,AAAA"}},
                    {"type":"input_text", "text":"describe"}
                ]
            }]
        });

        let converted = convert_responses_to_chat(
            &request,
            None,
            "mimo-v2.5",
            "mimo-v2.5",
            false,
            &ConversionDefaults::default(),
            ReasoningPolicy::default(),
        )
        .expect("conversion should succeed");

        let content = &converted.chat_messages[0]["content"];
        assert!(content.is_array());
        assert_eq!(content[0]["type"], "image_url");
        assert_eq!(content[0]["image_url"]["url"], "data:image/png;base64,AAAA");
        assert_eq!(content[1]["type"], "text");
    }

    #[test]
    fn normalizes_input_image_to_image_url() {
        let content = normalize_content(Some(&json!([
            {"type":"input_image", "image_url":"https://example.com/a.png"}
        ])))
        .expect("content should normalize");

        assert_eq!(content[0]["type"], "image_url");
        assert_eq!(content[0]["image_url"]["url"], "https://example.com/a.png");
    }

    #[test]
    fn preserves_input_audio_and_infers_format_from_data_url() {
        let content = normalize_content(Some(&json!([
            {"type":"input_audio", "input_audio":{"data":"data:audio/mpeg;base64,AAAA"}},
            {"type":"text", "text":"describe audio"}
        ])))
        .expect("content should normalize");

        assert_eq!(content[0]["type"], "input_audio");
        assert_eq!(
            content[0]["input_audio"]["data"],
            "data:audio/mpeg;base64,AAAA"
        );
        assert_eq!(content[0]["input_audio"]["format"], "mp3");
        assert_eq!(content[1]["type"], "text");
    }

    #[test]
    fn rejects_unsupported_audio_format() {
        let err = normalize_content(Some(&json!([
            {"type":"input_audio", "input_audio":{"data":"data:audio/aac;base64,AAAA"}}
        ])))
        .expect_err("aac should be rejected by the strict allowlist");

        assert!(
            err.to_string()
                .contains("unsupported input_audio MIME type")
        );
    }

    #[test]
    fn rejects_base64_strings_larger_than_50mb() {
        let too_large = "A".repeat(MAX_BASE64_STRING_BYTES + 1);
        let err = validate_audio_data(&too_large, Some("mp3"))
            .expect_err("oversized base64 should be rejected");

        assert!(err.to_string().contains("exceeds 50 MB"));
    }

    #[test]
    fn rejects_unknown_content_part_type() {
        let err = normalize_content(Some(&json!([
            {"type":"file", "file_id":"file_x"}
        ])))
        .expect_err("unknown part type should be rejected");

        assert!(err.to_string().contains("unsupported content part type"));
    }

    #[test]
    fn explicit_thinking_mode_inserts_thinking_option_and_empty_assistant_reasoning() {
        let request = json!({
            "model": "mimo-v2.5-pro",
            "input": "continue"
        });
        let previous_messages = vec![json!({"role":"assistant", "content":"earlier answer"})];
        let policy = ReasoningPolicy {
            thinking_enabled: true,
        };

        let converted = convert_responses_to_chat(
            &request,
            Some(previous_messages),
            "mimo-v2.5-pro",
            "mimo-v2.5-pro",
            false,
            &ConversionDefaults::default(),
            policy,
        )
        .expect("conversion should succeed");

        assert_eq!(converted.chat_payload["thinking"]["type"], json!("enabled"));
        assert_eq!(converted.chat_messages[0]["reasoning_content"], json!(""));
    }

    #[test]
    fn preserves_incoming_assistant_reasoning_content_when_enabled() {
        let request = json!({
            "model": "mimo-v2.5-pro",
            "input": [{
                "type": "message",
                "role": "assistant",
                "content": "tool plan",
                "reasoning_content": "provider reasoning"
            }]
        });
        let policy = ReasoningPolicy {
            thinking_enabled: true,
        };

        let converted = convert_responses_to_chat(
            &request,
            None,
            "mimo-v2.5-pro",
            "mimo-v2.5-pro",
            false,
            &ConversionDefaults::default(),
            policy,
        )
        .expect("conversion should succeed");

        assert_eq!(
            converted.chat_messages[0]["reasoning_content"],
            json!("provider reasoning")
        );
    }

    #[test]
    fn preserves_incoming_assistant_reasoning_content_without_explicit_thinking_option() {
        let request = json!({
            "model": "mimo-v2.5-pro",
            "input": [{
                "type": "message",
                "role": "assistant",
                "content": "tool plan",
                "reasoning_content": "provider reasoning"
            }]
        });
        let policy = ReasoningPolicy {
            thinking_enabled: false,
        };

        let converted = convert_responses_to_chat(
            &request,
            None,
            "mimo-v2.5-pro",
            "mimo-v2.5-pro",
            false,
            &ConversionDefaults::default(),
            policy,
        )
        .expect("conversion should succeed");

        assert_eq!(
            converted.chat_messages[0]["reasoning_content"],
            json!("provider reasoning")
        );
        assert!(converted.chat_payload.get("thinking").is_none());
    }

    #[test]
    fn replays_custom_tool_call_and_output_as_chat_tool_messages() {
        let request = json!({
            "model": "mimo-v2.5",
            "input": [
                {
                    "type": "custom_tool_call",
                    "call_id": "call_custom_1",
                    "name": "local_shell",
                    "input": "pwd"
                },
                {
                    "type": "custom_tool_call_output",
                    "call_id": "call_custom_1",
                    "output": "M:/CodeHub"
                }
            ]
        });

        let converted = convert_responses_to_chat(
            &request,
            None,
            "mimo-v2.5",
            "mimo-v2.5",
            false,
            &ConversionDefaults::default(),
            ReasoningPolicy::default(),
        )
        .expect("conversion should succeed");

        assert_eq!(converted.chat_messages.len(), 2);
        assert_eq!(converted.chat_messages[0]["role"], json!("assistant"));
        assert_eq!(
            converted.chat_messages[0]["tool_calls"][0]["function"]["name"],
            json!("local_shell")
        );
        assert_eq!(
            converted.chat_messages[0]["tool_calls"][0]["function"]["arguments"],
            json!("{\"input\":\"pwd\"}")
        );
        assert_eq!(converted.chat_messages[1]["role"], json!("tool"));
        assert_eq!(
            converted.chat_messages[1]["tool_call_id"],
            json!("call_custom_1")
        );
        assert_eq!(converted.chat_messages[1]["content"], json!("M:/CodeHub"));
    }
}
