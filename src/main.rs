mod config;
mod error;
mod responses;
mod xiaomi;

use std::net::SocketAddr;
use std::time::Instant;

use axum::body::Body;
use axum::extract::{ConnectInfo, OriginalUri, Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use config::AppConfig;
use error::ShimError;
use responses::build::build_response_object;
use responses::convert::{ConversionDefaults, convert_responses_to_chat};
use responses::sse::{buffered_sse_events, failed_sse_events};
use responses::store::{ResponseStore, StoredResponse};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tower_http::cors::CorsLayer;
use tracing::info;
use uuid::Uuid;
use xiaomi::client::XiaomiClient;
use xiaomi::reasoning::ReasoningPolicy;
use xiaomi::schema::ProviderTokenStats;

const VERSION_FLAG: &str = "--version";
const BUILD_INFO_FLAG: &str = "--build-info";

#[derive(Clone)]
struct AppState {
    config: AppConfig,
    xiaomi: XiaomiClient,
    store: ResponseStore,
}

struct CreatedResponse {
    response_object: Value,
    provider_ms: u128,
    observed_token_stats: ProviderTokenStats,
    client_model: String,
    provider_model: String,
    model_route: String,
}

#[derive(Debug, Clone)]
struct ResolvedModel {
    client_model: String,
    provider_model: String,
    route: &'static str,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(exit_code) = handle_process_metadata_command() {
        std::process::exit(exit_code);
    }

    let config = AppConfig::load()?;
    init_tracing(&config.log_level);
    let xiaomi = XiaomiClient::new(config.clone())?;
    let store = ResponseStore::new(config.response_store_max);
    let state = AppState {
        config: config.clone(),
        xiaomi,
        store,
    };

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/models", get(list_models))
        .route("/v1/responses", post(create_response))
        .route("/v1/responses/compact", post(compact_unsupported))
        .route("/v1/responses/{response_id}", get(get_response))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let listener = TcpListener::bind(config.bind).await?;
    info!(
        bind = %config.bind,
        config = ?config.config_source,
        "{}",
        config.startup_message
    );
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

fn init_tracing(filter: &str) {
    let filter = filter.to_string();
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .compact()
        .with_target(false)
        .init();
}

async fn healthz() -> Json<Value> {
    Json(json!({"status":"ok"}))
}

async fn list_models(State(state): State<AppState>) -> Json<Value> {
    let mut ids = Vec::new();
    for id in &state.config.mimo_models {
        append_unique_model_id(&mut ids, id);
    }
    for id in state.config.model_aliases.keys() {
        append_unique_model_id(&mut ids, id);
    }

    let data: Vec<Value> = ids
        .iter()
        .map(|id| json!({"id": id, "object": "model", "created": 0, "owned_by": "xiaomi-mimo"}))
        .collect();
    Json(json!({"object":"list", "data": data}))
}

fn append_unique_model_id(ids: &mut Vec<String>, id: &str) {
    if !id.trim().is_empty() && !ids.iter().any(|existing| existing == id) {
        ids.push(id.to_string());
    }
}

async fn compact_unsupported(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let started = Instant::now();
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let trace_id = resolve_trace_id(&payload, &headers);
    let request_model = extract_request_model(&payload);
    let path = uri.path().to_string();

    access_in(&state, addr, &path, stream, &trace_id);

    let err = ShimError::UnsupportedFeature {
        code: "compact_not_supported",
        message: "/v1/responses/compact is a Codex remote compaction control-plane request; codex-mimo-shim does not emulate compact and does not forward it to Xiaomi"
            .to_string(),
    };
    let status = err.status();
    let elapsed_ms = started.elapsed().as_millis();

    access_out(
        &state,
        addr,
        &path,
        stream,
        &trace_id,
        request_model.as_deref(),
        None,
        Some("unsupported_codex_control_plane_request"),
        None,
        0,
        elapsed_ms,
        status.as_u16(),
        Some(&err.to_string()),
    );

    err.into_response()
}

async fn get_response(
    State(state): State<AppState>,
    Path(response_id): Path<String>,
) -> Result<Json<Value>, ShimError> {
    let record = state
        .store
        .get(&response_id)
        .await
        .ok_or_else(|| ShimError::ResponseNotFound(response_id.clone()))?;
    Ok(Json(record.response_object))
}

async fn create_response(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let started = Instant::now();
    let stream = payload
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let trace_id = resolve_trace_id(&payload, &headers);
    let request_model = extract_request_model(&payload);
    let path = uri.path().to_string();
    access_in(&state, addr, &path, stream, &trace_id);

    let result = create_response_inner(state.clone(), payload, trace_id.clone()).await;
    let elapsed_ms = started.elapsed().as_millis();

    match result {
        Ok(created) => {
            access_out(
                &state,
                addr,
                &path,
                stream,
                &trace_id,
                Some(&created.client_model),
                Some(&created.provider_model),
                Some(&created.model_route),
                Some(&created.observed_token_stats),
                created.provider_ms,
                elapsed_ms,
                200,
                None,
            );
            if stream {
                sse_response(buffered_sse_events(&created.response_object))
            } else {
                Json(created.response_object).into_response()
            }
        }
        Err(err) => {
            let status = err.status();
            access_out(
                &state,
                addr,
                &path,
                stream,
                &trace_id,
                request_model.as_deref(),
                None,
                None,
                None,
                0,
                elapsed_ms,
                status.as_u16(),
                Some(&err.to_string()),
            );
            if stream {
                sse_response(failed_sse_events("resp_local_failed", &err))
            } else {
                err.into_response()
            }
        }
    }
}

async fn create_response_inner(
    state: AppState,
    payload: Value,
    trace_id: String,
) -> Result<CreatedResponse, ShimError> {
    let previous_id = payload.get("previous_response_id").and_then(Value::as_str);
    let previous_messages = if let Some(id) = previous_id {
        let record = state
            .store
            .get(id)
            .await
            .ok_or_else(|| ShimError::ResponseNotFound(id.to_string()))?;
        Some(record.chat_messages)
    } else {
        None
    };

    let requested_model = extract_request_model(&payload);
    let resolved_model = resolve_provider_model(&state.config, requested_model.as_deref())?;

    let defaults = ConversionDefaults {
        temperature: state.config.default_temperature,
        top_p: state.config.default_top_p,
        max_output_tokens: state.config.default_max_output_tokens,
    };
    let reasoning_policy = ReasoningPolicy {
        thinking_enabled: state.config.thinking_mode_enabled,
    };

    let converted = convert_responses_to_chat(
        &payload,
        previous_messages,
        &resolved_model.client_model,
        &resolved_model.provider_model,
        state.config.forward_parallel_tool_calls,
        &defaults,
        reasoning_policy,
    )?;

    let provider_started = Instant::now();
    let chat_result = state
        .xiaomi
        .chat_completions(converted.chat_payload.clone())
        .await?;
    let provider_ms = provider_started.elapsed().as_millis();
    let observed_token_stats = ProviderTokenStats::from_usage(chat_result.usage.as_ref());

    let mut built = build_response_object(
        &converted.response_id,
        &converted.client_model,
        &converted.chat_messages,
        &converted.tool_registry,
        chat_result,
        converted.parallel_tool_calls,
        converted.store,
        reasoning_policy,
    )?;

    if let Some(obj) = built
        .response_object
        .get_mut("local_gateway")
        .and_then(Value::as_object_mut)
    {
        obj.insert("trace_id".to_string(), json!(trace_id));
        obj.insert("provider_ms".to_string(), json!(provider_ms));
        obj.insert(
            "client_model".to_string(),
            json!(converted.client_model.clone()),
        );
        obj.insert(
            "provider_model".to_string(),
            json!(converted.provider_model.clone()),
        );
        obj.insert("model_route".to_string(), json!(resolved_model.route));
    }

    // Always store the minimal chain state. Codex often sends store=false while still
    // needing previous_response_id for function-call continuation.
    state
        .store
        .put(
            converted.response_id,
            StoredResponse {
                response_object: built.response_object.clone(),
                chat_messages: built.updated_chat_messages,
            },
        )
        .await;

    Ok(CreatedResponse {
        response_object: built.response_object,
        provider_ms,
        observed_token_stats,
        client_model: converted.client_model,
        provider_model: converted.provider_model,
        model_route: resolved_model.route.to_string(),
    })
}

fn sse_response(events: Vec<String>) -> Response {
    let body = events.concat();
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from(body))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

fn resolve_trace_id(payload: &Value, headers: &HeaderMap) -> String {
    extract_trace_id(payload, headers)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("TRC-{}", Uuid::new_v4().simple().to_string().to_uppercase()))
}

fn extract_trace_id(payload: &Value, headers: &HeaderMap) -> Option<String> {
    payload
        .get("metadata")
        .and_then(|metadata| metadata.get("trace_id"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .or_else(|| {
            payload
                .get("trace_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .get("metadata")
                .and_then(|metadata| metadata.get("request_id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .get("request_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            headers
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(ToOwned::to_owned)
        })
}

fn access_in(state: &AppState, _addr: SocketAddr, path: &str, stream: bool, trace_id: &str) {
    if !state.config.access_log {
        return;
    }
    info!(
        unstream = !stream,
        path = path,
        trace_id = trace_id,
        "{}",
        state.config.access_in_message
    );
}

fn access_out(
    state: &AppState,
    _addr: SocketAddr,
    path: &str,
    stream: bool,
    trace_id: &str,
    model: Option<&str>,
    provider_model: Option<&str>,
    model_route: Option<&str>,
    token_stats: Option<&ProviderTokenStats>,
    provider_ms: u128,
    total_ms: u128,
    status: u16,
    error: Option<&str>,
) {
    if !state.config.access_log {
        return;
    }
    let model = model.unwrap_or("-");
    let provider_model = provider_model.unwrap_or("-");
    let model_route = model_route.unwrap_or("-");
    let obs_usage = if token_stats
        .map(ProviderTokenStats::has_any)
        .unwrap_or(false)
    {
        "provider_usage"
    } else {
        "-"
    };
    let obs_input_tokens = format_optional_u64(token_stats.and_then(|stats| stats.input_tokens));
    let obs_output_tokens = format_optional_u64(token_stats.and_then(|stats| stats.output_tokens));
    let obs_total_tokens = format_optional_u64(token_stats.and_then(|stats| stats.total_tokens));
    let obs_cached_tokens = format_optional_u64(token_stats.and_then(|stats| stats.cached_tokens));
    let obs_reasoning_tokens =
        format_optional_u64(token_stats.and_then(|stats| stats.reasoning_tokens));
    info!(
        unstream = !stream,
        path = path,
        trace_id = trace_id,
        model = model,
        provider_model = provider_model,
        model_route = model_route,
        obs_usage = obs_usage,
        obs_input_tokens = obs_input_tokens.as_str(),
        obs_output_tokens = obs_output_tokens.as_str(),
        obs_total_tokens = obs_total_tokens.as_str(),
        obs_cached_tokens = obs_cached_tokens.as_str(),
        obs_reasoning_tokens = obs_reasoning_tokens.as_str(),
        provider_ms = provider_ms,
        total_ms = total_ms,
        status = status,
        error = error.unwrap_or("-"),
        "{}",
        state.config.access_out_message
    );
}

fn resolve_provider_model(
    config: &AppConfig,
    requested_model: Option<&str>,
) -> Result<ResolvedModel, ShimError> {
    let requested_model = requested_model.and_then(|model| {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let Some(client_model) = requested_model else {
        return Ok(ResolvedModel {
            client_model: config.mimo_model.clone(),
            provider_model: config.mimo_model.clone(),
            route: "default_model",
        });
    };

    if let Some(provider_model) = config.model_aliases.get(client_model) {
        let provider_model = provider_model.trim();
        if provider_model.is_empty() {
            return Err(ShimError::InvalidRequest(format!(
                "model alias for {client_model} is empty"
            )));
        }
        return Ok(ResolvedModel {
            client_model: client_model.to_string(),
            provider_model: provider_model.to_string(),
            route: "configured_alias",
        });
    }

    if config.mimo_models.iter().any(|model| model == client_model) {
        return Ok(ResolvedModel {
            client_model: client_model.to_string(),
            provider_model: client_model.to_string(),
            route: "direct_provider_model",
        });
    }

    if config.fallback_unknown_model_to_default {
        return Ok(ResolvedModel {
            client_model: client_model.to_string(),
            provider_model: config.mimo_model.clone(),
            route: "fallback_unknown_model_to_default",
        });
    }

    Err(ShimError::InvalidRequest(format!(
        "unsupported model {client_model}; configure upstream.aliases or enable upstream.fallback_unknown_model_to_default"
    )))
}

fn extract_request_model(payload: &Value) -> Option<String> {
    payload
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn format_optional_u64(value: Option<u64>) -> String {
    value
        .map(|number| number.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn handle_process_metadata_command() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    let flag = args.next()?;

    match flag.as_str() {
        VERSION_FLAG => {
            print_version();
            Some(0)
        }
        BUILD_INFO_FLAG => {
            print_build_info();
            Some(0)
        }
        _ => None,
    }
}

fn print_version() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    print_build_info();
}

fn print_build_info() {
    println!(
        "cargo_lock_sha256={}",
        option_env!("BUILD_CARGO_LOCK_SHA256").unwrap_or("unknown")
    );
    println!(
        "ci_workflow_sha256={}",
        option_env!("BUILD_CI_WORKFLOW_SHA256").unwrap_or("unknown")
    );
    println!(
        "public_build_rs_sha256={}",
        option_env!("BUILD_PUBLIC_BUILD_RS_SHA256").unwrap_or("unknown")
    );
}
