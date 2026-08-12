use std::collections::HashMap;

use serde_json::{Map, Value, json};

#[derive(Debug, Clone)]
pub struct ToolIdentity {
    pub original_name: String,
    pub namespace_name: Option<String>,
    pub original_tool_type: String,
}

#[derive(Debug, Clone, Default)]
pub struct ToolIdentityRegistry {
    by_flat_name: HashMap<String, ToolIdentity>,
}

impl ToolIdentityRegistry {
    fn register(&mut self, flat_name: String, identity: ToolIdentity) {
        self.by_flat_name.insert(flat_name, identity);
    }

    pub fn resolve(&self, flat_name: &str) -> Option<&ToolIdentity> {
        self.by_flat_name.get(flat_name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatToolConversion {
    pub tools: Vec<Value>,
    pub registry: ToolIdentityRegistry,
}

/// Convert Codex/Responses function, custom, and namespace tool declarations into
/// Chat Completions function tools. Unsupported built-in tools are dropped.
pub fn responses_tools_to_chat_tools(tools: Option<&Value>) -> ChatToolConversion {
    let Some(items) = tools.and_then(Value::as_array) else {
        return ChatToolConversion::default();
    };

    let mut conversion = ChatToolConversion::default();
    for tool in items {
        let Some(obj) = tool.as_object() else {
            continue;
        };
        convert_tool(obj, None, &mut conversion);
    }
    conversion
}

fn convert_tool(
    tool: &Map<String, Value>,
    namespace: Option<&str>,
    conversion: &mut ChatToolConversion,
) {
    match tool.get("type").and_then(Value::as_str) {
        Some("namespace") => {
            let namespace_name = tool.get("name").and_then(Value::as_str).unwrap_or("");
            let Some(children) = tool.get("tools").and_then(Value::as_array) else {
                return;
            };
            for child in children {
                if let Some(child) = child.as_object() {
                    convert_tool(child, Some(namespace_name), conversion);
                }
            }
        }
        Some("function") => {
            let function = tool
                .get("function")
                .and_then(Value::as_object)
                .unwrap_or(tool);
            let Some(original_name) = function.get("name").and_then(Value::as_str) else {
                return;
            };
            let flat_name = namespace
                .map(|name| join_namespace_tool_name(name, original_name))
                .unwrap_or_else(|| original_name.to_string());
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
                "name": flat_name,
                "description": description,
                "parameters": parameters
            });
            if function.get("strict").and_then(Value::as_bool) == Some(true) {
                converted["strict"] = json!(true);
            }
            conversion
                .tools
                .push(json!({"type": "function", "function": converted}));
            conversion.registry.register(
                flat_name,
                ToolIdentity {
                    original_name: original_name.to_string(),
                    namespace_name: namespace.map(ToOwned::to_owned),
                    original_tool_type: "function".to_string(),
                },
            );
        }
        Some("custom") if namespace.is_none() => {
            let Some(name) = tool.get("name").and_then(Value::as_str) else {
                return;
            };
            let description = tool
                .get("description")
                .cloned()
                .unwrap_or_else(|| json!(""));
            conversion.tools.push(json!({
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
            conversion.registry.register(
                name.to_string(),
                ToolIdentity {
                    original_name: name.to_string(),
                    namespace_name: None,
                    original_tool_type: "custom".to_string(),
                },
            );
        }
        _ => {}
    }
}

pub fn chat_tool_calls_to_responses_items(
    tool_calls: &[Value],
    registry: &ToolIdentityRegistry,
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
            let flat_name = function.get("name").and_then(Value::as_str).unwrap_or("");
            let identity = registry.resolve(flat_name);
            let response_name = identity
                .map(|identity| identity.original_name.as_str())
                .unwrap_or(flat_name);
            let arguments = normalize_tool_arguments(function.get("arguments"));
            let is_custom = identity
                .map(|identity| identity.original_tool_type == "custom")
                .unwrap_or(false);
            let mut item = if is_custom {
                json!({
                    "id": format!("ctc_local_{call_id}"),
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": response_name,
                    "input": extract_custom_input(&arguments)
                })
            } else {
                json!({
                    "id": format!("fc_local_{call_id}"),
                    "type": "function_call",
                    "status": "completed",
                    "call_id": call_id,
                    "name": response_name,
                    "arguments": arguments
                })
            };
            if let Some(namespace_name) =
                identity.and_then(|identity| identity.namespace_name.as_ref())
            {
                item["namespace"] = json!(namespace_name);
            }
            Some(item)
        })
        .collect()
}

pub fn response_function_call_item_to_chat_tool_call(item: &Value) -> Value {
    let call_id = item
        .get("call_id")
        .and_then(Value::as_str)
        .or_else(|| item.get("id").and_then(Value::as_str))
        .unwrap_or("call_local_unknown");
    let original_name = item.get("name").and_then(Value::as_str).unwrap_or("");
    let name = item
        .get("namespace")
        .and_then(Value::as_str)
        .map(|namespace| join_namespace_tool_name(namespace, original_name))
        .unwrap_or_else(|| original_name.to_string());
    let arguments = if item.get("type").and_then(Value::as_str) == Some("custom_tool_call") {
        json!({"input": item.get("input").and_then(Value::as_str).unwrap_or("")}).to_string()
    } else {
        normalize_tool_arguments(item.get("arguments"))
    };
    json!({
        "id": call_id,
        "type": "function",
        "function": {"name": name, "arguments": arguments}
    })
}

fn join_namespace_tool_name(namespace_name: &str, function_name: &str) -> String {
    let namespace_name = namespace_name.trim_end_matches('_');
    if namespace_name.is_empty() {
        function_name.to_string()
    } else {
        format!("{namespace_name}__{function_name}")
    }
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
    json!({"role": "assistant", "content": null, "tool_calls": tool_calls})
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
        "function": {"name": name, "arguments": arguments}
    })
}

pub fn make_tool_message(call_id: &str, output: &Value) -> Value {
    let content = match output {
        Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    };
    json!({"role": "tool", "tool_call_id": call_id, "content": content})
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
        assert_eq!(
            converted
                .registry
                .resolve("local_shell")
                .map(|tool| tool.original_tool_type.as_str()),
            Some("custom")
        );
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
        let converted = responses_tools_to_chat_tools(Some(&json!([{
            "type": "custom",
            "name": "local_shell"
        }])));
        let items = chat_tool_calls_to_responses_items(
            &[json!({
                "id": "call_123",
                "type": "function",
                "function": {
                    "name": "local_shell",
                    "arguments": "{\"input\":\"pwd\"}"
                }
            })],
            &converted.registry,
        );

        assert_eq!(items[0]["type"], json!("custom_tool_call"));
        assert_eq!(items[0]["input"], json!("pwd"));
    }

    #[test]
    fn flattens_namespace_tools_and_restores_identity() {
        let converted = responses_tools_to_chat_tools(Some(&json!([{
            "type": "namespace",
            "name": "terminal",
            "tools": [{
                "type": "function",
                "name": "exec",
                "parameters": {"type": "object", "properties": {}}
            }]
        }])));

        assert_eq!(
            converted.tools[0]["function"]["name"],
            json!("terminal__exec")
        );
        let items = chat_tool_calls_to_responses_items(
            &[json!({
                "id": "call_123",
                "type": "function",
                "function": {"name": "terminal__exec", "arguments": "{}"}
            })],
            &converted.registry,
        );
        assert_eq!(items[0]["name"], json!("exec"));
        assert_eq!(items[0]["namespace"], json!("terminal"));
    }

    #[test]
    fn replays_namespaced_custom_tool_calls_as_function_calls() {
        let tool_call = response_function_call_item_to_chat_tool_call(&json!({
            "id": "ctc_123",
            "type": "custom_tool_call",
            "call_id": "call_123",
            "name": "exec",
            "namespace": "terminal",
            "input": "pwd"
        }));

        assert_eq!(tool_call["type"], json!("function"));
        assert_eq!(tool_call["function"]["name"], json!("terminal__exec"));
        assert_eq!(
            tool_call["function"]["arguments"],
            json!("{\"input\":\"pwd\"}")
        );
    }
}
