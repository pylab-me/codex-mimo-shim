use serde_json::{Value, json};

use crate::error::ShimError;

pub fn buffered_sse_events(response: &Value) -> Vec<String> {
    let mut events = Vec::new();

    let empty_response = response_for_stream(response, "in_progress", Some(Vec::new()));
    events.push(sse(
        "response.created",
        json!({"type":"response.created", "response": empty_response}),
    ));
    let empty_response = response_for_stream(response, "in_progress", Some(Vec::new()));
    events.push(sse(
        "response.in_progress",
        json!({"type":"response.in_progress", "response": empty_response}),
    ));

    if let Some(output_items) = response.get("output").and_then(Value::as_array) {
        for (idx, item) in output_items.iter().enumerate() {
            events.push(sse(
                "response.output_item.added",
                json!({
                    "type": "response.output_item.added",
                    "output_index": idx,
                    "item": stream_added_item(item)
                }),
            ));

            match item.get("type").and_then(Value::as_str) {
                Some("message") => append_message_events(&mut events, idx, item),
                Some("function_call") => append_function_call_events(&mut events, idx, item),
                Some("custom_tool_call") => append_custom_tool_call_events(&mut events, idx, item),
                _ => {}
            }

            events.push(sse(
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "output_index": idx,
                    "item": item
                }),
            ));
        }
    }

    events.push(sse(
        "response.completed",
        json!({"type":"response.completed", "response": response}),
    ));
    events.push("data: [DONE]\n\n".to_string());
    events
}

fn response_for_stream(response: &Value, status: &str, output: Option<Vec<Value>>) -> Value {
    let mut envelope = response.clone();
    if let Some(obj) = envelope.as_object_mut() {
        obj.insert("status".to_string(), json!(status));
        if let Some(output) = output {
            obj.insert("output".to_string(), Value::Array(output));
        }
        if status != "completed" {
            obj.insert("usage".to_string(), Value::Null);
            obj.insert("output_text".to_string(), json!(""));
        }
    }
    envelope
}

fn stream_added_item(item: &Value) -> Value {
    let mut added = item.clone();
    let Some(obj) = added.as_object_mut() else {
        return added;
    };
    let item_type = obj
        .get("type")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    obj.insert("status".to_string(), json!("in_progress"));
    match item_type.as_deref() {
        Some("message") => {
            obj.insert("content".to_string(), Value::Array(Vec::new()));
        }
        Some("function_call") => {
            obj.insert("arguments".to_string(), json!(""));
        }
        Some("custom_tool_call") => {
            obj.insert("input".to_string(), json!(""));
        }
        _ => {}
    }
    added
}

fn append_custom_tool_call_events(events: &mut Vec<String>, output_index: usize, item: &Value) {
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
    let input = item.get("input").and_then(Value::as_str).unwrap_or("");
    if !input.is_empty() {
        events.push(sse(
            "response.custom_tool_call_input.delta",
            json!({
                "type": "response.custom_tool_call_input.delta",
                "item_id": item_id,
                "output_index": output_index,
                "delta": input
            }),
        ));
    }
    events.push(sse(
        "response.custom_tool_call_input.done",
        json!({
            "type": "response.custom_tool_call_input.done",
            "item_id": item_id,
            "output_index": output_index,
            "input": input
        }),
    ));
}

fn append_message_events(events: &mut Vec<String>, output_index: usize, item: &Value) {
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for (content_index, part) in content.iter().enumerate() {
        let mut added_part = part.clone();
        if let Some(obj) = added_part.as_object_mut() {
            if obj.get("type").and_then(Value::as_str) == Some("output_text") {
                obj.insert("text".to_string(), json!(""));
            }
        }
        events.push(sse(
            "response.content_part.added",
            json!({
                "type": "response.content_part.added",
                "item_id": item_id,
                "output_index": output_index,
                "content_index": content_index,
                "part": added_part
            }),
        ));
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            if !text.is_empty() {
                events.push(sse(
                    "response.output_text.delta",
                    json!({
                        "type": "response.output_text.delta",
                        "item_id": item_id,
                        "output_index": output_index,
                        "content_index": content_index,
                        "delta": text
                    }),
                ));
            }
            events.push(sse(
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": content_index,
                    "text": text
                }),
            ));
        }
        events.push(sse(
            "response.content_part.done",
            json!({
                "type": "response.content_part.done",
                "item_id": item_id,
                "output_index": output_index,
                "content_index": content_index,
                "part": part
            }),
        ));
    }
}

fn append_function_call_events(events: &mut Vec<String>, output_index: usize, item: &Value) {
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
    let arguments = normalize_arguments(item.get("arguments"));
    if !arguments.is_empty() {
        events.push(sse(
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": item_id,
                "output_index": output_index,
                "delta": arguments
            }),
        ));
    }
    events.push(sse(
        "response.function_call_arguments.done",
        json!({
            "type": "response.function_call_arguments.done",
            "item_id": item_id,
            "output_index": output_index,
            "arguments": arguments
        }),
    ));
}

fn normalize_arguments(arguments: Option<&Value>) -> String {
    match arguments {
        Some(Value::String(text)) => text.clone(),
        Some(other) => serde_json::to_string(other).unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

fn sse(event: &str, payload: Value) -> String {
    format!(
        "event: {event}\ndata: {}\n\n",
        serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string())
    )
}

pub fn failed_sse_events(response_id: &str, error: &ShimError) -> Vec<String> {
    let response = json!({
        "id": response_id,
        "object": "response",
        "created_at": time::OffsetDateTime::now_utc().unix_timestamp(),
        "status": "failed",
        "output": [],
        "output_text": "",
        "error": {
            "message": error.to_string(),
            "type": error.error_type(),
            "code": error.code()
        },
        "incomplete_details": null
    });
    vec![
        sse(
            "response.created",
            json!({"type":"response.created", "response": response}),
        ),
        sse(
            "response.failed",
            json!({"type":"response.failed", "response": response}),
        ),
        "data: [DONE]\n\n".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_custom_tool_input_events() {
        let response = json!({
            "id": "resp_test",
            "status": "completed",
            "output": [{
                "id": "ctc_local_call_1",
                "type": "custom_tool_call",
                "status": "completed",
                "call_id": "call_1",
                "name": "local_shell",
                "input": "pwd"
            }]
        });

        let events = buffered_sse_events(&response).join("");
        assert!(events.contains("response.custom_tool_call_input.delta"));
        assert!(events.contains("response.custom_tool_call_input.done"));
        assert!(events.contains("\"input\":\"pwd\""));
    }
}
