use serde_json::{Value, json};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::error::ShimError;
use crate::responses::tools::{
    ToolIdentityRegistry, chat_tool_calls_to_responses_items, make_assistant_tool_calls_message,
};
use crate::xiaomi::reasoning::{
    ReasoningPolicy, apply_reasoning_policy_to_assistant_message, extract_reasoning_content,
};
use crate::xiaomi::schema::{ChatResult, ProviderTokenStats};

#[derive(Debug, Clone)]
pub struct BuiltResponse {
    pub response_object: Value,
    pub updated_chat_messages: Vec<Value>,
}

pub fn build_response_object(
    response_id: &str,
    client_model: &str,
    base_chat_messages: &[Value],
    tool_registry: &ToolIdentityRegistry,
    chat_result: ChatResult,
    parallel_tool_calls: bool,
    store: bool,
    reasoning_policy: ReasoningPolicy,
) -> Result<BuiltResponse, ShimError> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let message = chat_result.message;
    let tool_calls = message
        .get("tool_calls")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut updated = base_chat_messages.to_vec();
    let reasoning_content = extract_reasoning_content(&message);
    let (output, output_text) = if !tool_calls.is_empty() {
        let mut assistant_message = make_assistant_tool_calls_message(tool_calls.clone());
        apply_reasoning_policy_to_assistant_message(
            &mut assistant_message,
            reasoning_content.as_deref(),
            reasoning_policy,
        );
        updated.push(assistant_message);
        (
            chat_tool_calls_to_responses_items(&tool_calls, tool_registry),
            String::new(),
        )
    } else {
        let text = message
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut assistant_message = json!({"role":"assistant", "content": text});
        apply_reasoning_policy_to_assistant_message(
            &mut assistant_message,
            reasoning_content.as_deref(),
            reasoning_policy,
        );
        updated.push(assistant_message);
        let item = json!({
            "id": format!("msg_local_{}", Uuid::new_v4().simple()),
            "type": "message",
            "status": "completed",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": text,
                "annotations": []
            }]
        });
        (vec![item], text)
    };

    let provider_usage = chat_result.usage.clone();
    let observed_token_stats = ProviderTokenStats::from_usage(provider_usage.as_ref());
    let responses_usage = normalize_responses_usage(provider_usage.as_ref())?;

    let mut response = json!({
        "id": response_id,
        "object": "response",
        "created_at": now,
        "status": "completed",
        "model": client_model,
        "output": output,
        "output_text": output_text,
        "error": null,
        "incomplete_details": null,
        "parallel_tool_calls": parallel_tool_calls,
        "store": store,
        // Codex parses response.completed as a Responses API object. The shim
        // converts provider token accounting only when all required counts are
        // present. It never fabricates input/output/total token counts.
        "usage": responses_usage
    });

    response["local_gateway"] = json!({
        "name": "codex-mimo-shim",
        "backend": "xiaomimimo",
        "endpoint_used": "chat.completions",
        "finish_reason": chat_result.finish_reason,
        "provider_raw_id": chat_result.raw.get("id").cloned().unwrap_or(Value::Null),
        "provider_usage": provider_usage.unwrap_or(Value::Null),
        "observed_token_stats": observed_token_stats.as_json()
    });

    Ok(BuiltResponse {
        response_object: response,
        updated_chat_messages: updated,
    })
}

fn normalize_responses_usage(provider_usage: Option<&Value>) -> Result<Value, ShimError> {
    let Some(usage) = provider_usage else {
        return Err(ShimError::ProviderProtocol(
            "missing_provider_usage: provider response is missing usage".to_string(),
        ));
    };

    let input_tokens = preferred_nonzero_number_field(usage, "input_tokens", "prompt_tokens")
        .ok_or_else(|| {
            ShimError::ProviderProtocol(
                "missing_provider_usage: usage is missing input_tokens/prompt_tokens".to_string(),
            )
        })?;
    let output_tokens = preferred_nonzero_number_field(usage, "output_tokens", "completion_tokens")
        .ok_or_else(|| {
            ShimError::ProviderProtocol(
                "missing_provider_usage: usage is missing output_tokens/completion_tokens"
                    .to_string(),
            )
        })?;
    let total_tokens = number_field(usage, &["total_tokens"]).ok_or_else(|| {
        ShimError::ProviderProtocol(
            "missing_provider_usage: usage is missing total_tokens".to_string(),
        )
    })?;

    let cached_tokens = nested_number_field(
        usage,
        &[
            ("input_tokens_details", "cached_tokens"),
            ("prompt_tokens_details", "cached_tokens"),
        ],
    )
    .unwrap_or(0);

    let reasoning_tokens = nested_number_field(
        usage,
        &[
            ("output_tokens_details", "reasoning_tokens"),
            ("completion_tokens_details", "reasoning_tokens"),
        ],
    )
    .unwrap_or(0);

    Ok(json!({
        "input_tokens": input_tokens,
        "input_tokens_details": {
            "cached_tokens": cached_tokens
        },
        "output_tokens": output_tokens,
        "output_tokens_details": {
            "reasoning_tokens": reasoning_tokens
        },
        "total_tokens": total_tokens
    }))
}

fn number_field(value: &Value, names: &[&str]) -> Option<u64> {
    names
        .iter()
        .find_map(|name| value.get(*name).and_then(Value::as_u64))
}

fn preferred_nonzero_number_field(value: &Value, primary: &str, fallback: &str) -> Option<u64> {
    let primary_value = value.get(primary).and_then(Value::as_u64);
    let fallback_value = value.get(fallback).and_then(Value::as_u64);
    match (primary_value, fallback_value) {
        (Some(0), Some(other)) if other > 0 => Some(other),
        (Some(current), _) => Some(current),
        (None, Some(other)) => Some(other),
        (None, None) => None,
    }
}

fn nested_number_field(value: &Value, names: &[(&str, &str)]) -> Option<u64> {
    names.iter().find_map(|(outer, inner)| {
        value
            .get(*outer)
            .and_then(|obj| obj.get(*inner))
            .and_then(Value::as_u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage() -> Value {
        json!({
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        })
    }

    #[test]
    fn stores_assistant_reasoning_content_for_text_response_when_enabled() {
        let chat_result = ChatResult {
            raw: json!({"id":"chatcmpl_test"}),
            message: json!({
                "role": "assistant",
                "content": "final answer",
                "reasoning_content": "provider reasoning"
            }),
            finish_reason: Some("stop".to_string()),
            usage: Some(usage()),
        };
        let policy = ReasoningPolicy {
            thinking_enabled: true,
        };

        let built = build_response_object(
            "resp_test",
            "mimo-v2.5-pro",
            &[],
            &ToolIdentityRegistry::default(),
            chat_result,
            false,
            true,
            policy,
        )
        .expect("response should build");

        assert_eq!(
            built.updated_chat_messages[0]["reasoning_content"],
            json!("provider reasoning")
        );
    }

    #[test]
    fn stores_assistant_reasoning_content_for_tool_calls_when_enabled() {
        let chat_result = ChatResult {
            raw: json!({"id":"chatcmpl_test"}),
            message: json!({
                "role": "assistant",
                "content": null,
                "reasoning_content": "provider reasoning",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "lookup", "arguments": "{}"}
                }]
            }),
            finish_reason: Some("tool_calls".to_string()),
            usage: Some(usage()),
        };
        let policy = ReasoningPolicy {
            thinking_enabled: true,
        };

        let built = build_response_object(
            "resp_test",
            "mimo-v2.5-pro",
            &[],
            &ToolIdentityRegistry::default(),
            chat_result,
            true,
            true,
            policy,
        )
        .expect("response should build");

        assert_eq!(
            built.updated_chat_messages[0]["reasoning_content"],
            json!("provider reasoning")
        );
        assert!(built.updated_chat_messages[0]["tool_calls"].is_array());
    }

    #[test]
    fn preserves_provider_reasoning_content_even_when_explicit_thinking_is_disabled() {
        let chat_result = ChatResult {
            raw: json!({"id":"chatcmpl_test"}),
            message: json!({
                "role": "assistant",
                "content": "final answer",
                "reasoning_content": "provider reasoning"
            }),
            finish_reason: Some("stop".to_string()),
            usage: Some(usage()),
        };
        let policy = ReasoningPolicy {
            thinking_enabled: false,
        };

        let built = build_response_object(
            "resp_test",
            "mimo-v2.5-pro",
            &[],
            &ToolIdentityRegistry::default(),
            chat_result,
            false,
            true,
            policy,
        )
        .expect("response should build");

        assert_eq!(
            built.updated_chat_messages[0]["reasoning_content"],
            json!("provider reasoning")
        );
    }

    #[test]
    fn prefers_openai_usage_fields_over_zeroed_nonstandard_fields() {
        let usage = json!({
            "input_tokens": 0,
            "output_tokens": 0,
            "prompt_tokens": 17467,
            "completion_tokens": 20,
            "total_tokens": 17487,
            "prompt_tokens_details": {
                "cached_tokens": 17408
            }
        });

        let normalized = normalize_responses_usage(Some(&usage)).expect("usage should normalize");
        let observed = ProviderTokenStats::from_usage(Some(&usage));

        assert_eq!(normalized["input_tokens"], json!(17467));
        assert_eq!(normalized["output_tokens"], json!(20));
        assert_eq!(observed.input_tokens, Some(17467));
        assert_eq!(observed.output_tokens, Some(20));
        assert_eq!(observed.cached_tokens, Some(17408));
    }

    #[test]
    fn keeps_nonstandard_usage_fields_when_they_have_real_values() {
        let usage = json!({
            "input_tokens": 321,
            "output_tokens": 45,
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 366
        });

        let normalized = normalize_responses_usage(Some(&usage)).expect("usage should normalize");
        let observed = ProviderTokenStats::from_usage(Some(&usage));

        assert_eq!(normalized["input_tokens"], json!(321));
        assert_eq!(normalized["output_tokens"], json!(45));
        assert_eq!(observed.input_tokens, Some(321));
        assert_eq!(observed.output_tokens, Some(45));
    }

    #[test]
    fn restores_provider_function_tool_call_as_custom_tool_call() {
        let chat_result = ChatResult {
            raw: json!({"id":"chatcmpl_test"}),
            message: json!({
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_custom_1",
                    "type": "function",
                    "function": {
                        "name": "local_shell",
                        "arguments": "{\"input\":\"pwd\"}"
                    }
                }]
            }),
            finish_reason: Some("tool_calls".to_string()),
            usage: Some(usage()),
        };

        let built = build_response_object(
            "resp_test",
            "mimo-v2.5",
            &[],
            &crate::responses::tools::responses_tools_to_chat_tools(Some(&json!([{
                "type": "custom",
                "name": "local_shell"
            }])))
            .registry,
            chat_result,
            true,
            true,
            ReasoningPolicy::default(),
        )
        .expect("response should build");

        assert_eq!(
            built.response_object["output"][0]["type"],
            json!("custom_tool_call")
        );
        assert_eq!(
            built.response_object["output"][0]["name"],
            json!("local_shell")
        );
        assert_eq!(built.response_object["output"][0]["input"], json!("pwd"));
    }
}
