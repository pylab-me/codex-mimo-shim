# Features / 功能

## English

`codex-mimo-shim` provides a local compatibility layer that enables `Codex` to use Xiaomi MiMo.

- Also tested on LongCat.

### Core features

```text
- Local /v1/responses endpoint
- Local /v1/responses/{response_id} endpoint
- Local /v1/models endpoint
- (Experimental) /v1/responses/compact
- Text-only request conversion
- Image support
- Codex pseudo-model passthrough in Codex desktop
- Responses-style input to Chat Completions messages conversion
- Chat Completions result to Responses-style output wrapping
- Non-streaming response mode
- SSE streaming response mode
- Local response_store support
- Local previous_response_id continuation
- Tool-call event translation for supported function_call flows
- function_call and function_call_output handling
- Chat tool_calls history reconstruction
- Strict provider usage mapping
- Provider usage validation
- Access logs with trace_id, model, latency, status, and observed token stats
- Configurable upstream base_url, api_key, model, and model list
- Release transparency metadata support
```

### API facade features

```text
POST /v1/responses
GET  /v1/responses/{response_id}
GET  /v1/models
```

### Supported request fields

```text
model
input
instructions
temperature
top_p
max_output_tokens
stream
store
previous_response_id
```

### Input support

```text
input: string
input: list of message items
message roles: user / assistant / system / developer
text content parts
```

### Streaming support

```text
- response.created
- response.in_progress
- response.output_item.added
- response.output_item.done
- response.content_part.added
- response.output_text.delta
- response.output_text.done
- response.function_call_arguments.delta
- response.function_call_arguments.done
- response.completed
- response.failed
```

### Tool-call support

```text
- function_call output item conversion
- function_call arguments streaming
- function_call_output input handling
- assistant tool_calls reconstruction
- tool message reconstruction for Chat Completions upstream
```

### Usage mapping

```text
prompt_tokens      -> input_tokens
completion_tokens  -> output_tokens
total_tokens       -> total_tokens
```

### Observability features

```text
- trace_id per request
- upstream latency logging
- total request latency logging
- request model logging
- status logging
- provider error logging
- non-standard observed token stats in access logs
```

### Release transparency features

```text
- Public Cargo.lock publishing
- Cargo.lock SHA256 publishing
- Release artifact SHA256 publishing
- build-info.json support
- Public release workflow support
- Public build.rs metadata injector support
- Optional executable build metadata support
```

---

## 中文

`codex-mimo-shim` 提供一个本地兼容层，令 `Codex` 可以使用小米 MiMo。

### 核心功能

```text
- 本地 /v1/responses endpoint
- 本地 /v1/responses/{response_id} endpoint
- 本地 /v1/models endpoint
- text-only 请求转换
- 图片/音频输入支持，默认对大于50M的数据进行提示
- Responses-style input 转 Chat Completions messages
- Chat Completions result 包装为 Responses-style output
- 非流式响应模式
- SSE 流式响应模式
- 本地 response_store 支持
- 本地 previous_response_id 续接
- 支持部分 function_call 工具事件转换
- function_call 与 function_call_output 处理
- Chat tool_calls 历史重建
- 严格 provider usage 映射
- provider usage 校验
- access log 输出 trace_id、model、耗时、状态和观测 token 统计
- 可配置 upstream base_url、api_key、model、model list
- 支持 release transparency metadata
```

### API facade 功能

```text
POST /v1/responses
GET  /v1/responses/{response_id}
GET  /v1/models
```

### 支持的请求字段

```text
model
input
instructions
temperature
top_p
max_output_tokens
stream
store
previous_response_id
```

### Input 支持

```text
input: 字符串
input: message item 数组
message roles: user / assistant / system / developer
text content parts
```

### 流式响应支持

```text
- response.created
- response.in_progress
- response.output_item.added
- response.output_item.done
- response.content_part.added
- response.output_text.delta
- response.output_text.done
- response.function_call_arguments.delta
- response.function_call_arguments.done
- response.completed
- response.failed
```

### 工具调用支持

```text
- function_call output item 转换
- function_call arguments streaming
- function_call_output input 处理
- assistant tool_calls 重建
- 面向 Chat Completions 上游的 tool message 重建
```

### Usage 映射

```text
prompt_tokens      -> input_tokens
completion_tokens  -> output_tokens
total_tokens       -> total_tokens
```

### 可观测性功能

```text
- 每个请求生成 trace_id
- 上游请求耗时日志
- 总请求耗时日志
- 请求 model 日志
- 状态日志
- provider error 日志
- access log 输出非标准观测 token 统计
```

### 发布透明度功能

```text
- 支持公开 Cargo.lock
- 支持公开 Cargo.lock SHA256
- 支持公开 release artifact SHA256
- 支持 build-info.json
- 支持公开 release workflow
- 支持公开 build.rs metadata injector
- 支持可选 exe 内嵌 build metadata
```
