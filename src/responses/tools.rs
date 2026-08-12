use std::collections::HashSet;

use serde_json::{Value, json};

#[derive(Debug, Clone, Default)]
pub struct ChatToolConversion {
    pub tools: Vec<Value>,
    pub custom_tool_names: HashSet<String>,
}

/// Convert Codex/Responses function/custom tool declarations into Chat Completions tools.
/// Built-in tools are still dropped because this shim only forwards client-owned schemas
/// to Xiaomi MiMo.
pub fn responses_tools_to_chat_tools(tools: Option<&Value>) -> ChatToolConversion {
    let Some(items) = tools.and_then(Value::as_array) else {
        return ChatToolConversion::default();
    };

    let mut chat_tools = Vec::new();
    let mut custom_tool_names = HashSet::new();

    for tool in items {
        let Some(obj) = tool.as_object() else {
            continue;
        };

        match obj.get("type").and_then(Value::as_str) {
            Some("function") => {
                let function = obj
                    .get("function")
                    .and_then(Value::as_object)
                    .unwrap_or(obj);
                let Some(name) = function.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let description = function
                    .get("description")
                    .cloned()
                    .unwrap_or_else(|| json!(""));
                let parameters = function
                    .get("parameters")
                    .filter(|value| value.is_object())
                    .cloned()
                    .unwrap_or_else(|| json!({"type":"object","properties":{}}));
                let mut converted = json!({
                    "name": name,
                    "description": description,
                    "parameters": parameters
                });
                if function.get("strict").and_then(Value::as_bool) == Some(true) {
                    converted["strict"] = json!(true);
                }
                chat_tools.push(json!({"type": "function", "function": converted}));
            }
            Some("custom") => {
                let Some(name) = obj.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let description = obj.get("description").cloned().unwrap_or_else(|| json!(""));
                chat_tools.push(json!({
                    "type": "function",
                    "function": {
                        "name": name,
                        "description": description,
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "input": {
                                    "type": "string",
                                    "description": "Raw custom tool input."
                                }
                            },
                            "required": ["input"],
                            "additionalProperties": false
                        }
                    }
                }));
                custom_tool_names.insert(name.to_string());
            }
            _ => {}
        }
    }

    ChatToolConversion {
        tools: chat_tools,
        custom_tool_names,
    }
}

pub fn chat_tool_calls_to_responses_items(
    tool_calls: &[Value],
    custom_tool_names: &HashSet<String>,
) -> Vec<Value> {
    tool_calls
        .iter()
        .filter_map(|call| {
            let obj = call.as_object()?;
            let call_id = obj
                .get("id")
                .and_then(Value::as_str)
                .or_else(|| obj.get("call_id").and_then(Value::as_str))
                .unwrap_or("call_local_unknown");
            let function = obj.get("function").and_then(Value::as_object)?;
            let name = function.get("name").and_then(Value::as_str).unwrap_or("");
            let arguments = normalize_tool_arguments(function.get("arguments"));
            if custom_tool_names.contains(name) {
                Some(json!({
                    "id": format!("ctc_local_{call_id}"),
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": name,
                    "input": extract_custom_input(&arguments)
                }))
            } else {
                Some(json!({
                    "id": format!("fc_local_{call_id}"),
                    "type": "function_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": name,
                    "arguments": arguments
                }))
            }
        })
        .collect()
}

pub fn response_function_call_item_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .or_else(|| item.get("id").and_then(Value::as_str))
        .unwrap_or("call_local_unknown");
    let name = item.get("name").and_then(Value::as_str).unwrap_or("");
    let arguments = if item.get("type").and_then(Value::as_str) == Some("custom_tool_call") {
        json!({
            "input": item.get("input").and_then(Value::as_str).unwrap_or("")
        })
        .to_string()
    } else {
        normalize_tool_arguments(item.get("arguments"))
    };
    json!({
        "id": call_id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments
        }
    })
}

fn normalize_tool_arguments(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(text)) => text.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

fn extract_custom_input(arguments: &str) -> String {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| value.get("input").cloned())
        .and_then(|value| match value {
            Value::String(text) => Some(text),
            Value::Null => Some(String::new()),
            other => Some(other.to_string()),
        })
        .unwrap_or_default()
}

pub fn make_assistant_tool_calls_message(tool_calls: Vec<Value>) -> Value {
    let tool_calls = tool_calls
        .into_iter()
        .map(normalize_chat_tool_call)
        .collect::<Vec<_>>();
    json!({
        "role": "assistant",
        "content": null,
        "tool_calls": tool_calls
    })
}

fn normalize_chat_tool_call(call: Value) -> Value {
    let Some(obj) = call.as_object() else {
        return call;
    };

    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .or_else(|| obj.get("call_id").and_then(Value::as_str))
        .unwrap_or("call_local_unknown");
    let function = obj.get("function").and_then(Value::as_object);
    let name = function
        .and_then(|function| function.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments =
        normalize_tool_arguments(function.and_then(|function| function.get("arguments")));

    json!({
        "id": id,
        "type": "function",
        "function": {
            "name": name,
            "arguments": arguments
        }
    })
}

pub fn make_tool_message(call_id: &str, output: &Value) -> Value {
    let content = match output {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    };
    json!({
        "role": "tool",
        "tool_call_id": call_id,
        "content": content
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forwards_custom_tools_as_single_input_functions() {
        let converted = responses_tools_to_chat_tools(Some(&json!([
            {
                "type": "custom",
                "name": "local_shell",
                "description": "Run a shell command."
            }
        ])));

        assert_eq!(converted.tools.len(), 1);
        assert!(converted.custom_tool_names.contains("local_shell"));
        assert_eq!(
            converted.tools[0]["function"]["parameters"]["required"],
            json!(["input"])
        );
        assert_eq!(
            converted.tools[0]["function"]["parameters"]["properties"]["input"]["type"],
            json!("string")
        );
    }

    #[test]
    fn restores_custom_tool_calls_from_chat_arguments() {
        let custom_tool_names = HashSet::from([String::from("local_shell")]);
        let items = chat_tool_calls_to_responses_items(
            &[json!({
                "id": "call_123",
                "type": "function",
                "function": {
                    "name": "local_shell",
                    "arguments": "{\"input\":\"pwd\"}"
                }
            })],
            &custom_tool_names,
        );

        assert_eq!(items[0]["type"], json!("custom_tool_call"));
        assert_eq!(items[0]["input"], json!("pwd"));
    }

    #[test]
    fn replays_custom_tool_calls_as_function_calls() {
        let tool_call = response_function_call_item_to_chat_tool_call(&json!({
            "id": "ctc_123",
            "type": "custom_tool_call",
            "call_id": "call_123",
            "name": "local_shell",
            "input": "pwd"
        }));

        assert_eq!(tool_call["type"], json!("function"));
        assert_eq!(tool_call["function"]["name"], json!("local_shell"));
        assert_eq!(
            tool_call["function"]["arguments"],
            json!("{\"input\":\"pwd\"}")
        );
    }
}
