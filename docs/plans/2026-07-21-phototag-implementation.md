# phototag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `phototag-server` (a stateless HTTP service that turns an image into a keyword list via a local vision LLM) and `phototag-watch` (a filesystem watcher + backfill tool that finds untagged images, calls `phototag-server`, and writes the returned keywords into the file's own IPTC/XMP metadata via `exiftool`).

**Architecture:** Three-crate Cargo workspace (`common`, `server`, `watch`), each with a `lib.rs` exposing its modules and a thin `main.rs`/`[[bin]]` — mirrors `find-anything`'s `crates/`-per-binary layout so tests construct real components in-process (an axum router, a mock HTTP server) rather than mocking at a framework level. `phototag-server` has no filesystem access; `phototag-watch` has no LLM-calling logic of its own — all image understanding goes through `phototag-server`.

**Tech Stack:** Rust, `axum` 0.8 + `tokio` (server), `reqwest` 0.13 (rustls) for both outbound HTTP paths, `notify` 8 for filesystem watching, `walkdir` for backfill, `clap` 4 (derive + env) for CLI/config, `serde` + `toml` for `phototag-watch`'s config file, `tracing` for logging, `exiftool` invoked as a subprocess for metadata writes.

---

## Before you start

Install `exiftool` on this dev machine — it's required by `crates/watch`'s tests from Task 6 onward, and isn't installed here yet (only confirmed on the target NAS via Entware):

```bash
sudo apt-get install -y libimage-exiftool-perl
exiftool -ver
```

Expected: prints a version number (e.g. `13.xx`).

---

## Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml` (workspace root)
- Create: `crates/common/Cargo.toml`
- Create: `crates/common/src/lib.rs`
- Create: `crates/server/Cargo.toml`
- Create: `crates/server/src/lib.rs`
- Create: `crates/server/src/main.rs`
- Create: `crates/watch/Cargo.toml`
- Create: `crates/watch/src/lib.rs`
- Create: `crates/watch/src/main.rs`
- Create: `.gitignore`
- Generated: `Cargo.lock` (written by `cargo build` in Step 6 — committed since this workspace produces binaries, not just a library)

This task is pure scaffolding (no behavior yet), so it's verified by a successful build rather than a test.

- [ ] **Step 1: Create the workspace root `Cargo.toml`**

```toml
[workspace]
members = [
    "crates/common",
    "crates/server",
    "crates/watch",
]
resolver = "2"

[workspace.dependencies]
anyhow = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "net", "time", "process", "sync", "signal"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
toml = "0.8"
clap = { version = "4", features = ["derive", "env"] }
reqwest = { version = "0.13", features = ["json", "rustls"], default-features = false }
tempfile = "3"
```

- [ ] **Step 2: Create `crates/common/Cargo.toml` and stub `lib.rs`**

```toml
[package]
name = "phototag-common"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { workspace = true }

[lib]
name = "phototag_common"
path = "src/lib.rs"
```

```rust
// crates/common/src/lib.rs
```

- [ ] **Step 3: Create `crates/server/Cargo.toml` and stub `lib.rs`/`main.rs`**

```toml
[package]
name = "phototag-server"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "phototag-server"
path = "src/main.rs"

[lib]
name = "phototag_server"
path = "src/lib.rs"

[dependencies]
phototag-common = { path = "../common" }
anyhow = { workspace = true }
axum = "0.8"
base64 = "0.22"
clap = { workspace = true }
reqwest = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

```rust
// crates/server/src/lib.rs
```

```rust
// crates/server/src/main.rs
fn main() {}
```

- [ ] **Step 4: Create `crates/watch/Cargo.toml` and stub `lib.rs`/`main.rs`**

```toml
[package]
name = "phototag-watch"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "phototag-watch"
path = "src/main.rs"

[lib]
name = "phototag_watch"
path = "src/lib.rs"

[dependencies]
phototag-common = { path = "../common" }
anyhow = { workspace = true }
clap = { workspace = true }
notify = "8"
reqwest = { workspace = true }
serde = { workspace = true }
toml = { workspace = true }
tokio = { workspace = true }
tracing = { workspace = true }
tracing-subscriber = { workspace = true }
walkdir = "2"

[dev-dependencies]
phototag-server = { path = "../server" }
axum = "0.8"
image = { version = "0.25", default-features = false, features = ["jpeg"] }
serde_json = { workspace = true }
tempfile = { workspace = true }
tokio = { workspace = true, features = ["rt-multi-thread", "macros"] }
```

```rust
// crates/watch/src/lib.rs
```

```rust
// crates/watch/src/main.rs
fn main() {}
```

- [ ] **Step 5: Create `.gitignore`**

```
/target
```

- [ ] **Step 6: Build the workspace**

Run: `cargo build --workspace`
Expected: `Compiling phototag-common v0.1.0 ...` then `Compiling phototag-server v0.1.0 ...` then `Compiling phototag-watch v0.1.0 ...` then `Finished` with no errors.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates .gitignore
git commit -m "Scaffold phototag Cargo workspace"
```

---

## Task 2: `phototag-common` — shared `TagResponse` type

**Files:**
- Modify: `crates/common/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/common/src/lib.rs
use serde::{Deserialize, Serialize};

/// Response body for `phototag-server`'s `POST /tag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagResponse {
    pub keywords: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_response_round_trips_through_json() {
        let original = TagResponse {
            keywords: vec!["dog".to_string(), "beach".to_string()],
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: TagResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, original);
    }
}
```

This needs `serde_json` as a dev-dependency, which isn't in `crates/common/Cargo.toml` yet.

- [ ] **Step 2: Add the dev-dependency**

```toml
# crates/common/Cargo.toml — add:
[dev-dependencies]
serde_json = { workspace = true }
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p phototag-common`
Expected: compile error — `serde_json` not yet a workspace-visible dev-dep before Step 2, or if run after Step 2, the test itself should already pass since the type above is complete. Run it once before adding `TagResponse`'s derives to confirm — since this is a small type, write the struct without `#[cfg(test)]` module first, run `cargo test -p phototag-common` and expect `error[E0433]: failed to resolve: use of undeclared crate or module `serde_json`` if the dev-dependency is missing, confirming the test harness is wired up.

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p phototag-common`
Expected: `test tests::tag_response_round_trips_through_json ... ok`

- [ ] **Step 5: Commit**

```bash
git add crates/common
git commit -m "Add shared TagResponse type"
```

---

## Task 3: `phototag-server` — LLM gateway client

**Files:**
- Create: `crates/server/src/config.rs`
- Create: `crates/server/src/llm_client.rs`
- Create: `crates/server/tests/helpers/mod.rs`
- Create: `crates/server/tests/llm_client.rs`
- Modify: `crates/server/src/lib.rs`

The gateway client builds an OpenAI-compatible multimodal chat-completion request, sends it via `reqwest`, and parses the model's text response into a clean keyword list. Tested against a real (tiny) mock HTTP server rather than a mocking framework, matching `find-anything`'s own `TestServer`-in-process testing convention.

- [ ] **Step 1: Write `config.rs`**

```rust
// crates/server/src/config.rs
use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "phototag-server")]
pub struct ServerConfig {
    /// Address to bind the HTTP server to.
    #[arg(long, env = "PHOTOTAG_LISTEN_ADDR", default_value = "0.0.0.0:8080")]
    pub listen_addr: String,

    /// Base URL of an OpenAI-compatible chat-completions gateway, e.g.
    /// `http://gateway:8080/v1` — `/chat/completions` is appended.
    #[arg(long, env = "PHOTOTAG_GATEWAY_URL")]
    pub gateway_url: String,

    /// Model name to send in the chat-completion request.
    #[arg(long, env = "PHOTOTAG_GATEWAY_MODEL")]
    pub gateway_model: String,

    /// Request timeout, in seconds. Generous by default since the gateway
    /// may need to wake a sleeping GPU host before it can respond.
    #[arg(long, env = "PHOTOTAG_GATEWAY_TIMEOUT_SECS", default_value_t = 120)]
    pub gateway_timeout_secs: u64,

    /// Overrides the built-in keyword-extraction prompt.
    #[arg(long, env = "PHOTOTAG_PROMPT")]
    pub prompt: Option<String>,
}
```

- [ ] **Step 2: Write the failing test for `llm_client`**

```rust
// crates/server/tests/helpers/mod.rs
#![allow(dead_code)]

use axum::{routing::post, Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

/// Starts a tiny axum server that mimics an OpenAI-compatible
/// `/chat/completions` endpoint, always replying with `content` as the
/// assistant message text. Returns its base URL (no trailing slash).
pub async fn spawn_mock_gateway(content: &str) -> String {
    let content = content.to_string();
    let app = Router::new().route(
        "/chat/completions",
        post(move || {
            let content = content.clone();
            async move {
                Json(json!({
                    "choices": [
                        { "message": { "content": content } }
                    ]
                }))
            }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock gateway");
    });
    format!("http://{addr}")
}

/// Same as `spawn_mock_gateway`, but the endpoint always returns a 500.
pub async fn spawn_failing_gateway() -> String {
    let app = Router::new().route(
        "/chat/completions",
        post(|| async { (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "boom") }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve failing gateway");
    });
    format!("http://{addr}")
}
```

```rust
// crates/server/tests/llm_client.rs
mod helpers;

use helpers::{spawn_failing_gateway, spawn_mock_gateway};
use phototag_server::config::ServerConfig;
use phototag_server::llm_client::GatewayClient;

fn test_config(gateway_url: String) -> ServerConfig {
    ServerConfig {
        listen_addr: "127.0.0.1:0".into(),
        gateway_url,
        gateway_model: "test-model".into(),
        gateway_timeout_secs: 5,
        prompt: None,
    }
}

#[tokio::test]
async fn extracts_keywords_from_comma_separated_response() {
    let base_url = spawn_mock_gateway("dog, beach, sunset").await;
    let client = GatewayClient::new(&test_config(base_url));

    let keywords = client
        .extract_keywords(b"fake-image-bytes", "image/jpeg")
        .await
        .expect("extract_keywords");

    assert_eq!(keywords, vec!["dog", "beach", "sunset"]);
}

#[tokio::test]
async fn extracts_keywords_from_json_array_response() {
    let base_url = spawn_mock_gateway(r#"["dog", "beach", "sunset"]"#).await;
    let client = GatewayClient::new(&test_config(base_url));

    let keywords = client
        .extract_keywords(b"fake-image-bytes", "image/jpeg")
        .await
        .expect("extract_keywords");

    assert_eq!(keywords, vec!["dog", "beach", "sunset"]);
}

#[tokio::test]
async fn errors_when_gateway_request_fails() {
    let base_url = spawn_failing_gateway().await;
    let client = GatewayClient::new(&test_config(base_url));

    let result = client.extract_keywords(b"fake-image-bytes", "image/jpeg").await;

    assert!(result.is_err());
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p phototag-server --test llm_client`
Expected: FAIL — `error[E0433]: failed to resolve: could not find `llm_client` in `phototag_server`` (module doesn't exist yet), and `error[E0433] ... could not find config` similarly.

- [ ] **Step 4: Implement `llm_client.rs`**

```rust
// crates/server/src/llm_client.rs
use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::config::ServerConfig;

pub const DEFAULT_PROMPT: &str = "List 3 to 8 concise keywords describing the objects, \
scene, and setting visible in this image. Respond with ONLY a comma-separated list of \
lowercase keywords and nothing else — no numbering, no sentences, no explanation.";

#[derive(Clone)]
pub struct GatewayClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    prompt: String,
}

impl GatewayClient {
    pub fn new(config: &ServerConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(config.gateway_timeout_secs))
            .build()
            .expect("building reqwest client");
        Self {
            http,
            base_url: config.gateway_url.trim_end_matches('/').to_string(),
            model: config.gateway_model.clone(),
            prompt: config
                .prompt
                .clone()
                .unwrap_or_else(|| DEFAULT_PROMPT.to_string()),
        }
    }

    pub async fn extract_keywords(&self, image_bytes: &[u8], content_type: &str) -> Result<Vec<String>> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_bytes);
        let data_url = format!("data:{content_type};base64,{b64}");

        let request = ChatRequest {
            model: &self.model,
            messages: vec![ChatMessage {
                role: "user",
                content: vec![
                    ContentPart::Text {
                        text: self.prompt.clone(),
                    },
                    ContentPart::ImageUrl {
                        image_url: ImageUrl { url: data_url },
                    },
                ],
            }],
        };

        let response = self
            .http
            .post(format!("{}/chat/completions", self.base_url))
            .json(&request)
            .send()
            .await
            .context("sending request to LLM gateway")?
            .error_for_status()
            .context("LLM gateway returned an error status")?;

        let parsed: ChatCompletionResponse = response
            .json()
            .await
            .context("parsing LLM gateway response as JSON")?;

        let content = parsed
            .choices
            .into_iter()
            .next()
            .context("LLM gateway response had no choices")?
            .message
            .content;

        let keywords = parse_keywords(&content);
        if keywords.is_empty() {
            bail!("no keywords could be parsed from LLM response: {content:?}");
        }
        Ok(keywords)
    }
}

fn parse_keywords(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();

    // The model sometimes answers with a JSON array despite the prompt
    // asking for a plain comma-separated list; try that first.
    if let Ok(list) = serde_json::from_str::<Vec<String>>(trimmed) {
        return clean_keywords(list);
    }

    let cleaned = trimmed.trim_start_matches('[').trim_end_matches(']');
    let words = cleaned
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect();
    clean_keywords(words)
}

fn clean_keywords(list: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .filter(|k| seen.insert(k.to_lowercase()))
        .collect()
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: Vec<ContentPart>,
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    content: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keywords_handles_comma_separated_text() {
        assert_eq!(
            parse_keywords("dog, beach, sunset"),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_handles_json_array() {
        assert_eq!(
            parse_keywords(r#"["dog", "beach", "sunset"]"#),
            vec!["dog", "beach", "sunset"]
        );
    }

    #[test]
    fn parse_keywords_dedupes_case_insensitively() {
        assert_eq!(parse_keywords("Dog, dog, DOG"), vec!["Dog"]);
    }

    #[test]
    fn parse_keywords_drops_empty_entries() {
        assert_eq!(parse_keywords("dog, , beach,"), vec!["dog", "beach"]);
    }
}
```

- [ ] **Step 5: Wire `config` and `llm_client` into `lib.rs`**

```rust
// crates/server/src/lib.rs
pub mod config;
pub mod llm_client;
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p phototag-server`
Expected: all `llm_client::tests::*` unit tests and all `tests/llm_client.rs` integration tests report `ok`.

- [ ] **Step 7: Commit**

```bash
git add crates/server
git commit -m "Add phototag-server LLM gateway client"
```

---

## Task 4: `phototag-server` — `POST /tag` endpoint

**Files:**
- Create: `crates/server/src/tag.rs`
- Modify: `crates/server/src/lib.rs`
- Modify: `crates/server/src/main.rs`
- Create: `crates/server/tests/tag_endpoint.rs`

- [ ] **Step 1: Write the failing integration test**

```rust
// crates/server/tests/tag_endpoint.rs
mod helpers;

use helpers::{spawn_failing_gateway, spawn_mock_gateway};
use phototag_common::TagResponse;
use phototag_server::config::ServerConfig;
use phototag_server::{build_router, create_app_state};
use tokio::net::TcpListener;

async fn spawn_phototag_server(gateway_url: String) -> String {
    let config = ServerConfig {
        listen_addr: "127.0.0.1:0".into(),
        gateway_url,
        gateway_model: "test-model".into(),
        gateway_timeout_secs: 5,
        prompt: None,
    };
    let state = create_app_state(config).await.expect("create_app_state");
    let app = build_router(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve phototag-server");
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn tag_endpoint_returns_keywords_on_success() {
    let gateway_url = spawn_mock_gateway("dog, beach, sunset").await;
    let server_url = spawn_phototag_server(gateway_url).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{server_url}/tag"))
        .header("content-type", "image/jpeg")
        .body(vec![0xff, 0xd8, 0xff]) // not a real JPEG — the server never decodes it
        .send()
        .await
        .expect("POST /tag");

    assert_eq!(response.status(), 200);
    let body: TagResponse = response.json().await.expect("parse response");
    assert_eq!(body.keywords, vec!["dog", "beach", "sunset"]);
}

#[tokio::test]
async fn tag_endpoint_returns_bad_gateway_when_upstream_fails() {
    let gateway_url = spawn_failing_gateway().await;
    let server_url = spawn_phototag_server(gateway_url).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{server_url}/tag"))
        .header("content-type", "image/jpeg")
        .body(vec![0xff, 0xd8, 0xff])
        .send()
        .await
        .expect("POST /tag");

    assert_eq!(response.status(), 502);
}

#[tokio::test]
async fn tag_endpoint_rejects_empty_body() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;

    let client = reqwest::Client::new();
    let response = client
        .post(format!("{server_url}/tag"))
        .body(Vec::<u8>::new())
        .send()
        .await
        .expect("POST /tag");

    assert_eq!(response.status(), 400);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p phototag-server --test tag_endpoint`
Expected: FAIL — `error[E0432]: unresolved import phototag_server::build_router` (doesn't exist yet).

- [ ] **Step 3: Implement `tag.rs`**

```rust
// crates/server/src/tag.rs
use axum::{
    body::Bytes,
    extract::State,
    http::{header::CONTENT_TYPE, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use phototag_common::TagResponse;
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn tag_handler(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: "empty request body".into(),
            }),
        )
            .into_response();
    }

    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();

    match state.gateway.extract_keywords(&body, &content_type).await {
        Ok(keywords) => (StatusCode::OK, Json(TagResponse { keywords })).into_response(),
        Err(e) => {
            tracing::warn!("tag request failed: {e:#}");
            (
                StatusCode::BAD_GATEWAY,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
                .into_response()
        }
    }
}
```

- [ ] **Step 4: Wire `AppState`, `create_app_state`, `build_router` into `lib.rs`**

```rust
// crates/server/src/lib.rs
pub mod config;
pub mod llm_client;
pub mod tag;

use axum::{extract::DefaultBodyLimit, routing::post, Router};
use anyhow::Result;

use config::ServerConfig;
use llm_client::GatewayClient;

#[derive(Clone)]
pub struct AppState {
    pub gateway: GatewayClient,
}

pub async fn create_app_state(config: ServerConfig) -> Result<AppState> {
    Ok(AppState {
        gateway: GatewayClient::new(&config),
    })
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/tag", post(tag::tag_handler))
        // Default axum body limit (2MB) is too small for real photos.
        .layer(DefaultBodyLimit::max(25 * 1024 * 1024))
        .with_state(state)
}
```

- [ ] **Step 5: Implement `main.rs`**

```rust
// crates/server/src/main.rs
use clap::Parser;
use phototag_server::config::ServerConfig;
use phototag_server::{build_router, create_app_state};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = ServerConfig::parse();
    let listen_addr = config.listen_addr.clone();

    let state = create_app_state(config).await?;
    let app = build_router(state);

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!("phototag-server listening on {listen_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p phototag-server`
Expected: all tests `ok`, including the three new `tag_endpoint` tests.

- [ ] **Step 7: Verify the binary actually starts**

Run:
```bash
PHOTOTAG_GATEWAY_URL=http://127.0.0.1:1 PHOTOTAG_GATEWAY_MODEL=test cargo run -p phototag-server &
sleep 1
curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:8080/tag --data-binary "not-empty"
kill %1
```
Expected: prints `502` (gateway URL is unreachable, but the server itself started and routed the request correctly), then the background server is killed.

- [ ] **Step 8: Commit**

```bash
git add crates/server
git commit -m "Add phototag-server POST /tag endpoint"
```

---

## Task 5: `phototag-server` — Dockerfile

**Files:**
- Create: `Dockerfile`
- Create: `.dockerignore`

- [ ] **Step 1: Create `.dockerignore`**

```
/target
.git
```

- [ ] **Step 2: Create the multi-stage Dockerfile**

```dockerfile
# Dockerfile
FROM rust:1.97-bookworm AS build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p phototag-server

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /build/target/release/phototag-server /usr/local/bin/phototag-server
ENV PHOTOTAG_LISTEN_ADDR=0.0.0.0:8080
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/phototag-server"]
```

- [ ] **Step 3: Build the image**

Run: `docker build -t phototag-server:local .`
Expected: build completes successfully, ending with `Successfully tagged phototag-server:local` (or the buildkit equivalent `naming to docker.io/library/phototag-server:local`).

If Docker isn't available in this environment, skip this verification step but keep the Dockerfile — it'll be verified when the image is actually deployed.

- [ ] **Step 4: Smoke-test the container**

Run:
```bash
docker run --rm -d -p 18080:8080 \
  -e PHOTOTAG_GATEWAY_URL=http://127.0.0.1:1 \
  -e PHOTOTAG_GATEWAY_MODEL=test \
  --name phototag-server-smoketest \
  phototag-server:local
sleep 1
curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:18080/tag --data-binary "not-empty"
docker stop phototag-server-smoketest
```
Expected: prints `502`, container stops cleanly.

- [ ] **Step 5: Commit**

```bash
git add Dockerfile .dockerignore
git commit -m "Add phototag-server Dockerfile"
```

---

## Task 6: `phototag-watch` — `exiftool` wrapper

**Files:**
- Create: `crates/watch/src/exif.rs`
- Modify: `crates/watch/src/lib.rs`

Requires `exiftool` on `PATH` (see "Before you start" at the top of this plan).

- [ ] **Step 1: Write the failing test**

```rust
// crates/watch/src/exif.rs
use std::path::Path;

use anyhow::{bail, Context, Result};
use tokio::process::Command;

/// Returns true if the file already has a non-empty `IPTC:Keywords` value.
pub async fn has_keywords(path: &Path) -> Result<bool> {
    let output = Command::new("exiftool")
        .arg("-IPTC:Keywords")
        .arg("-s3")
        .arg(path)
        .output()
        .await
        .context("running exiftool -IPTC:Keywords")?;

    if !output.status.success() {
        bail!(
            "exiftool exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

/// Writes `keywords` into the file's `IPTC:Keywords` and `XMP-dc:Subject`
/// fields in place (no `_original` backup file).
pub async fn write_keywords(path: &Path, keywords: &[String]) -> Result<()> {
    if keywords.is_empty() {
        bail!("refusing to write an empty keyword list to {}", path.display());
    }

    let mut cmd = Command::new("exiftool");
    cmd.arg("-overwrite_original");
    for kw in keywords {
        cmd.arg(format!("-IPTC:Keywords={kw}"));
        cmd.arg(format!("-XMP-dc:Subject={kw}"));
    }
    cmd.arg(path);

    let output = cmd.output().await.context("running exiftool to write keywords")?;
    if !output.status.success() {
        bail!(
            "exiftool exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_jpeg(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let img = image::RgbImage::new(4, 4);
        img.save(&path).expect("save test jpeg");
        path
    }

    #[tokio::test]
    async fn fresh_image_has_no_keywords() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "fresh.jpg");

        assert!(!has_keywords(&path).await.unwrap());
    }

    #[tokio::test]
    async fn write_then_read_round_trips_keywords() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "tagged.jpg");

        write_keywords(&path, &["dog".to_string(), "beach".to_string()])
            .await
            .unwrap();

        assert!(has_keywords(&path).await.unwrap());
    }

    #[tokio::test]
    async fn write_keywords_rejects_empty_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = make_test_jpeg(&dir, "empty.jpg");

        let result = write_keywords(&path, &[]).await;

        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Wire `exif` into `lib.rs`**

```rust
// crates/watch/src/lib.rs
pub mod exif;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p phototag-watch exif::`
Expected: compile error initially (`image`/`tempfile` not yet added as dev-dependencies — they already are, from Task 1's Cargo.toml — so this should instead fail only if `exiftool` is missing from `PATH`, in which case you'll see `No such file or directory (os error 2)` inside the `Context` message. Install it per "Before you start" if so.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p phototag-watch exif::`
Expected: `test exif::tests::fresh_image_has_no_keywords ... ok`, `test exif::tests::write_then_read_round_trips_keywords ... ok`, `test exif::tests::write_keywords_rejects_empty_list ... ok`

- [ ] **Step 5: Commit**

```bash
git add crates/watch
git commit -m "Add phototag-watch exiftool wrapper"
```

---

## Task 7: `phototag-watch` — config file parsing

**Files:**
- Create: `crates/watch/src/config.rs`
- Modify: `crates/watch/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/watch/src/config.rs
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server_url: String,
    #[serde(default)]
    pub roots: Vec<RootConfig>,
    #[serde(default)]
    pub watch: WatchSettings,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RootConfig {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WatchSettings {
    pub extensions: Vec<String>,
    pub debounce_ms: u64,
}

impl Default for WatchSettings {
    fn default() -> Self {
        WatchSettings {
            extensions: vec![
                "jpg".into(),
                "jpeg".into(),
                "png".into(),
                "tiff".into(),
                "heic".into(),
            ],
            debounce_ms: 2000,
        }
    }
}

impl WatchSettings {
    /// True if `path`'s extension (case-insensitive) is in the allow-list.
    pub fn matches_extension(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| {
                let e = e.to_lowercase();
                self.extensions.iter().any(|allowed| allowed.to_lowercase() == e)
            })
            .unwrap_or(false)
    }
}

pub fn load_config(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading config file {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing config file {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_TOML: &str = r#"
server_url = "http://phototag-server:8080"

[[roots]]
name = "pictures"
path = "/path/to/pictures"

[[roots]]
name = "second-library"
path = "/path/to/other/photos"

[watch]
extensions = ["jpg", "jpeg", "png", "tiff", "heic"]
debounce_ms = 2000
"#;

    #[test]
    fn parses_multiple_roots() {
        let config: Config = toml::from_str(EXAMPLE_TOML).unwrap();

        assert_eq!(config.server_url, "http://phototag-server:8080");
        assert_eq!(config.roots.len(), 2);
        assert_eq!(config.roots[0].name, "pictures");
        assert_eq!(config.roots[0].path, PathBuf::from("/path/to/pictures"));
        assert_eq!(config.roots[1].name, "second-library");
        assert_eq!(config.watch.debounce_ms, 2000);
    }

    #[test]
    fn watch_settings_default_when_omitted() {
        let toml = r#"
server_url = "http://phototag-server:8080"

[[roots]]
name = "pictures"
path = "/path/to/pictures"
"#;
        let config: Config = toml::from_str(toml).unwrap();

        assert_eq!(config.watch.debounce_ms, 2000);
        assert!(config.watch.extensions.contains(&"jpg".to_string()));
    }

    #[test]
    fn matches_extension_is_case_insensitive() {
        let settings = WatchSettings::default();

        assert!(settings.matches_extension(Path::new("photo.JPG")));
        assert!(settings.matches_extension(Path::new("photo.heic")));
        assert!(!settings.matches_extension(Path::new("document.pdf")));
    }
}
```

- [ ] **Step 2: Wire `config` into `lib.rs`**

```rust
// crates/watch/src/lib.rs
pub mod config;
pub mod exif;
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `cargo test -p phototag-watch config::`
Expected: FAIL to compile before this step's code exists (module didn't exist). After adding the code above, this should already pass — write the struct definitions first without running, then run once to confirm green.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p phototag-watch config::`
Expected: `test config::tests::parses_multiple_roots ... ok`, `test config::tests::watch_settings_default_when_omitted ... ok`, `test config::tests::matches_extension_is_case_insensitive ... ok`

- [ ] **Step 5: Commit**

```bash
git add crates/watch
git commit -m "Add phototag-watch TOML config parsing"
```

---

## Task 8: `phototag-watch` — HTTP client to `phototag-server`

**Files:**
- Create: `crates/watch/src/client.rs`
- Create: `crates/watch/tests/helpers/mod.rs`
- Create: `crates/watch/tests/client.rs`
- Modify: `crates/watch/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/watch/tests/helpers/mod.rs
#![allow(dead_code)]

use axum::{routing::post, Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

/// Starts a tiny axum server mimicking `phototag-server`'s `POST /tag`,
/// always replying with `keywords`.
pub async fn spawn_mock_phototag_server(keywords: &[&str]) -> String {
    let body = json!({ "keywords": keywords });
    let app = Router::new().route(
        "/tag",
        post(move || {
            let body = body.clone();
            async move { Json(body) }
        }),
    );

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve mock phototag-server");
    });
    format!("http://{addr}")
}
```

```rust
// crates/watch/tests/client.rs
mod helpers;

use helpers::spawn_mock_phototag_server;
use phototag_watch::client::TaggerClient;

fn make_test_jpeg(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let img = image::RgbImage::new(4, 4);
    img.save(&path).expect("save test jpeg");
    path
}

#[tokio::test]
async fn tag_image_returns_keywords_from_server() {
    let server_url = spawn_mock_phototag_server(&["dog", "beach"]).await;
    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");

    let client = TaggerClient::new(server_url);
    let keywords = client.tag_image(&path).await.unwrap();

    assert_eq!(keywords, vec!["dog".to_string(), "beach".to_string()]);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p phototag-watch --test client`
Expected: FAIL — `error[E0432]: unresolved import phototag_watch::client`.

- [ ] **Step 3: Implement `client.rs`**

```rust
// crates/watch/src/client.rs
use std::path::Path;

use anyhow::{bail, Context, Result};
use phototag_common::TagResponse;

pub struct TaggerClient {
    http: reqwest::Client,
    server_url: String,
}

impl TaggerClient {
    pub fn new(server_url: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .expect("building reqwest client");
        Self { http, server_url }
    }

    pub async fn tag_image(&self, path: &Path) -> Result<Vec<String>> {
        let bytes = tokio::fs::read(path)
            .await
            .with_context(|| format!("reading {}", path.display()))?;
        let content_type = content_type_for(path);
        let url = format!("{}/tag", self.server_url.trim_end_matches('/'));

        let response = self
            .http
            .post(&url)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(bytes)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            bail!("phototag-server returned {status}: {body}");
        }

        let parsed: TagResponse = response
            .json()
            .await
            .context("parsing phototag-server response")?;
        Ok(parsed.keywords)
    }
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "tiff" => "image/tiff",
        "heic" => "image/heic",
        _ => "application/octet-stream",
    }
}
```

- [ ] **Step 4: Wire `client` into `lib.rs`**

```rust
// crates/watch/src/lib.rs
pub mod client;
pub mod config;
pub mod exif;
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p phototag-watch --test client`
Expected: `test tag_image_returns_keywords_from_server ... ok`

- [ ] **Step 6: Commit**

```bash
git add crates/watch
git commit -m "Add phototag-watch HTTP client for phototag-server"
```

---

## Task 9: `phototag-watch` — tagging pipeline

**Files:**
- Create: `crates/watch/src/pipeline.rs`
- Modify: `crates/watch/src/lib.rs`
- Create: `crates/watch/tests/pipeline.rs`

This ties `exif` + `client` together and is the function both the watcher and the backfill walker will call per file. Its test uses the *real* `phototag-server` (a dev-dependency, per `crates/watch/Cargo.toml` from Task 1) pointed at a mock LLM gateway, giving genuine end-to-end coverage of the whole chain except the real LLM. This "spin up a real phototag-server against a mock gateway" combo is also what Tasks 10 and 11 need, so it goes into the shared `tests/helpers/mod.rs` now rather than being copy-pasted three times.

- [ ] **Step 1: Add gateway/server spawn helpers to `tests/helpers/mod.rs`**

```rust
// crates/watch/tests/helpers/mod.rs — add:
use phototag_server::config::ServerConfig;
use phototag_server::{build_router, create_app_state};

/// Starts a tiny axum server mimicking an OpenAI-compatible
/// `/chat/completions` endpoint, always replying with `content`.
pub async fn spawn_mock_gateway(content: &str) -> String {
    let content = content.to_string();
    let app = Router::new().route(
        "/chat/completions",
        post(move || {
            let content = content.clone();
            async move {
                Json(json!({
                    "choices": [{ "message": { "content": content } }]
                }))
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve mock gateway") });
    format!("http://{addr}")
}

/// Starts a real `phototag-server` pointed at `gateway_url`. Returns its base URL.
pub async fn spawn_phototag_server(gateway_url: String) -> String {
    let config = ServerConfig {
        listen_addr: "127.0.0.1:0".into(),
        gateway_url,
        gateway_model: "test-model".into(),
        gateway_timeout_secs: 5,
        prompt: None,
    };
    let state = create_app_state(config).await.expect("create_app_state");
    let app = build_router(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    tokio::spawn(async move { axum::serve(listener, app).await.expect("serve phototag-server") });
    format!("http://{addr}")
}
```

- [ ] **Step 2: Write the failing test**

```rust
// crates/watch/tests/pipeline.rs
mod helpers;

use helpers::{spawn_mock_gateway, spawn_phototag_server};
use phototag_watch::client::TaggerClient;
use phototag_watch::exif;
use phototag_watch::pipeline::{tag_one_file, TagOutcome};

fn make_test_jpeg(dir: &tempfile::TempDir, name: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    image::RgbImage::new(4, 4).save(&path).expect("save test jpeg");
    path
}

#[tokio::test]
async fn tags_a_fresh_image_end_to_end() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");

    let outcome = tag_one_file(&path, &client).await.unwrap();

    match outcome {
        TagOutcome::Tagged(keywords) => assert_eq!(keywords, vec!["dog", "beach"]),
        TagOutcome::AlreadyTagged => panic!("expected Tagged, got AlreadyTagged"),
    }
    assert!(exif::has_keywords(&path).await.unwrap());
}

#[tokio::test]
async fn skips_an_already_tagged_image() {
    let gateway_url = spawn_mock_gateway("should-not-be-called").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let dir = tempfile::TempDir::new().unwrap();
    let path = make_test_jpeg(&dir, "photo.jpg");
    exif::write_keywords(&path, &["existing".to_string()]).await.unwrap();

    let outcome = tag_one_file(&path, &client).await.unwrap();

    assert!(matches!(outcome, TagOutcome::AlreadyTagged));
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p phototag-watch --test pipeline`
Expected: FAIL — `error[E0432]: unresolved import phototag_watch::pipeline`.

- [ ] **Step 4: Implement `pipeline.rs`**

```rust
// crates/watch/src/pipeline.rs
use std::path::Path;

use anyhow::Result;

use crate::client::TaggerClient;
use crate::exif;

#[derive(Debug)]
pub enum TagOutcome {
    Tagged(Vec<String>),
    AlreadyTagged,
}

/// Tags a single file: skips it if it already has keywords, otherwise
/// calls `phototag-server` and writes the result into the file's metadata.
pub async fn tag_one_file(path: &Path, client: &TaggerClient) -> Result<TagOutcome> {
    if exif::has_keywords(path).await? {
        return Ok(TagOutcome::AlreadyTagged);
    }
    let keywords = client.tag_image(path).await?;
    exif::write_keywords(path, &keywords).await?;
    Ok(TagOutcome::Tagged(keywords))
}
```

- [ ] **Step 5: Wire `pipeline` into `lib.rs`**

```rust
// crates/watch/src/lib.rs
pub mod client;
pub mod config;
pub mod exif;
pub mod pipeline;
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p phototag-watch --test pipeline`
Expected: `test tags_a_fresh_image_end_to_end ... ok`, `test skips_an_already_tagged_image ... ok`

- [ ] **Step 7: Commit**

```bash
git add crates/watch
git commit -m "Add phototag-watch tagging pipeline"
```

---

## Task 10: `phototag-watch` — backfill mode

**Files:**
- Create: `crates/watch/src/backfill.rs`
- Modify: `crates/watch/src/lib.rs`
- Create: `crates/watch/tests/backfill.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/watch/tests/backfill.rs
mod helpers;

use helpers::{spawn_mock_gateway, spawn_phototag_server};
use phototag_watch::backfill::run_backfill;
use phototag_watch::client::TaggerClient;
use phototag_watch::config::{Config, RootConfig, WatchSettings};
use phototag_watch::exif;

fn make_test_jpeg(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    image::RgbImage::new(4, 4).save(&path).expect("save test jpeg");
    path
}

#[tokio::test]
async fn backfill_tags_untagged_files_across_multiple_roots() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root_a = tempfile::TempDir::new().unwrap();
    let root_b = tempfile::TempDir::new().unwrap();
    let photo_a = make_test_jpeg(root_a.path(), "a.jpg");
    let photo_b = make_test_jpeg(root_b.path(), "b.jpg");
    let ignored = root_a.path().join("notes.txt");
    std::fs::write(&ignored, b"not an image").unwrap();

    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![
            RootConfig { name: "a".into(), path: root_a.path().to_path_buf() },
            RootConfig { name: "b".into(), path: root_b.path().to_path_buf() },
        ],
        watch: WatchSettings::default(),
    };

    run_backfill(&config, &client, None).await.unwrap();

    assert!(exif::has_keywords(&photo_a).await.unwrap());
    assert!(exif::has_keywords(&photo_b).await.unwrap());
}

#[tokio::test]
async fn backfill_can_be_restricted_to_a_single_named_root() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root_a = tempfile::TempDir::new().unwrap();
    let root_b = tempfile::TempDir::new().unwrap();
    let photo_a = make_test_jpeg(root_a.path(), "a.jpg");
    let photo_b = make_test_jpeg(root_b.path(), "b.jpg");

    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![
            RootConfig { name: "a".into(), path: root_a.path().to_path_buf() },
            RootConfig { name: "b".into(), path: root_b.path().to_path_buf() },
        ],
        watch: WatchSettings::default(),
    };

    run_backfill(&config, &client, Some("a")).await.unwrap();

    assert!(exif::has_keywords(&photo_a).await.unwrap());
    assert!(!exif::has_keywords(&photo_b).await.unwrap());
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p phototag-watch --test backfill`
Expected: FAIL — `error[E0432]: unresolved import phototag_watch::backfill`.

- [ ] **Step 3: Implement `backfill.rs`**

```rust
// crates/watch/src/backfill.rs
use anyhow::Result;
use walkdir::WalkDir;

use crate::client::TaggerClient;
use crate::config::{Config, RootConfig};
use crate::pipeline::{tag_one_file, TagOutcome};

/// Walks each configured root once, tagging every file that doesn't yet
/// have keywords. If `only_root` is set, only that named root is walked.
pub async fn run_backfill(config: &Config, client: &TaggerClient, only_root: Option<&str>) -> Result<()> {
    for root in &config.roots {
        if let Some(name) = only_root {
            if root.name != name {
                continue;
            }
        }
        tracing::info!(root = %root.name, path = %root.path.display(), "backfill starting");
        backfill_root(root, &config.watch, client).await;
    }
    Ok(())
}

async fn backfill_root(root: &RootConfig, watch: &crate::config::WatchSettings, client: &TaggerClient) {
    for entry in WalkDir::new(&root.path).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if !watch.matches_extension(path) {
            continue;
        }
        match tag_one_file(path, client).await {
            Ok(TagOutcome::Tagged(keywords)) => {
                tracing::info!(path = %path.display(), ?keywords, "tagged");
            }
            Ok(TagOutcome::AlreadyTagged) => {
                tracing::debug!(path = %path.display(), "already tagged, skipping");
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "tagging failed, skipping");
            }
        }
    }
}
```

- [ ] **Step 4: Wire `backfill` into `lib.rs`**

```rust
// crates/watch/src/lib.rs
pub mod backfill;
pub mod client;
pub mod config;
pub mod exif;
pub mod pipeline;
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p phototag-watch --test backfill`
Expected: `test backfill_tags_untagged_files_across_multiple_roots ... ok`, `test backfill_can_be_restricted_to_a_single_named_root ... ok`

- [ ] **Step 6: Commit**

```bash
git add crates/watch
git commit -m "Add phototag-watch backfill mode"
```

---

## Task 11: `phototag-watch` — filesystem watcher

**Files:**
- Create: `crates/watch/src/watcher.rs`
- Modify: `crates/watch/src/lib.rs`
- Create: `crates/watch/tests/watcher.rs`

Follows `find-watch`'s established convention of reacting to `Create`/`Modify(Data)` events and using a debounce window as the "file is done being written" signal, rather than chasing platform-specific close-write events.

- [ ] **Step 1: Write the failing test**

```rust
// crates/watch/tests/watcher.rs
mod helpers;

use std::time::Duration;

use helpers::{spawn_mock_gateway, spawn_phototag_server};
use phototag_watch::client::TaggerClient;
use phototag_watch::config::{Config, RootConfig, WatchSettings};
use phototag_watch::exif;
use phototag_watch::watcher::run_watch;

#[tokio::test]
async fn watcher_tags_a_newly_created_file() {
    let gateway_url = spawn_mock_gateway("dog, beach").await;
    let server_url = spawn_phototag_server(gateway_url).await;
    let client = TaggerClient::new(server_url);

    let root = tempfile::TempDir::new().unwrap();
    let config = Config {
        server_url: "unused-in-this-test".into(),
        roots: vec![RootConfig {
            name: "root".into(),
            path: root.path().to_path_buf(),
        }],
        watch: WatchSettings {
            debounce_ms: 100,
            ..WatchSettings::default()
        },
    };

    let watch_handle = tokio::spawn(run_watch(config, client));

    // Give the watcher a moment to register its inotify watch before we
    // create the file, then write the image.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let path = root.path().join("new-photo.jpg");
    image::RgbImage::new(4, 4).save(&path).unwrap();

    // Poll for up to 5 seconds — comfortably longer than the 100ms debounce
    // plus the mock gateway round-trip.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if exif::has_keywords(&path).await.unwrap_or(false) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!("file was not tagged within 5 seconds");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    watch_handle.abort();
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p phototag-watch --test watcher`
Expected: FAIL — `error[E0432]: unresolved import phototag_watch::watcher`.

- [ ] **Step 3: Implement `watcher.rs`**

```rust
// crates/watch/src/watcher.rs
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::client::TaggerClient;
use crate::config::Config;
use crate::pipeline::tag_one_file;

/// Watches all configured roots and tags new/changed files as they settle.
/// Runs until the event channel closes (i.e. forever, in normal operation) —
/// callers that need to stop it early should abort the task it's spawned in.
pub async fn run_watch(config: Config, client: TaggerClient) -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();

    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(event) = res else { return };
        let is_relevant = matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Modify(notify::event::ModifyKind::Data(_))
                | EventKind::Modify(notify::event::ModifyKind::Any)
        );
        if !is_relevant {
            return;
        }
        for path in event.paths {
            let _ = tx.send(path);
        }
    })
    .context("creating filesystem watcher")?;

    for root in &config.roots {
        watcher
            .watch(&root.path, RecursiveMode::Recursive)
            .with_context(|| format!("watching root '{}' ({})", root.name, root.path.display()))?;
        tracing::info!(root = %root.name, path = %root.path.display(), "watching");
    }

    let debounce = Duration::from_millis(config.watch.debounce_ms);
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    loop {
        let sleep_for = pending
            .values()
            .min()
            .map(|deadline| deadline.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));

        tokio::select! {
            maybe_path = rx.recv() => {
                let Some(path) = maybe_path else { break };
                if !config.watch.matches_extension(&path) {
                    continue;
                }
                pending.insert(path, Instant::now() + debounce);
            }
            _ = tokio::time::sleep(sleep_for), if !pending.is_empty() => {
                let now = Instant::now();
                let ready: Vec<PathBuf> = pending
                    .iter()
                    .filter(|(_, deadline)| **deadline <= now)
                    .map(|(path, _)| path.clone())
                    .collect();
                for path in ready {
                    pending.remove(&path);
                    if !path.is_file() {
                        continue;
                    }
                    match tag_one_file(&path, &client).await {
                        Ok(outcome) => tracing::info!(path = %path.display(), ?outcome, "processed"),
                        Err(e) => tracing::warn!(path = %path.display(), error = %e, "tagging failed, skipping"),
                    }
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 4: Wire `watcher` into `lib.rs`**

```rust
// crates/watch/src/lib.rs
pub mod backfill;
pub mod client;
pub mod config;
pub mod exif;
pub mod pipeline;
pub mod watcher;
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p phototag-watch --test watcher`
Expected: `test watcher_tags_a_newly_created_file ... ok`

If this is flaky in CI due to inotify timing, re-run once — the 5-second poll window is generous, but a heavily loaded CI runner is the most likely cause of an occasional miss.

- [ ] **Step 6: Commit**

```bash
git add crates/watch
git commit -m "Add phototag-watch filesystem watcher"
```

---

## Task 12: `phototag-watch` — CLI wiring

**Files:**
- Modify: `crates/watch/src/main.rs`

- [ ] **Step 1: Implement `main.rs`**

```rust
// crates/watch/src/main.rs
use std::path::PathBuf;

use clap::Parser;
use phototag_watch::{backfill, client::TaggerClient, config, watcher};

/// Watches configured photo library roots and tags new images, or
/// (with --backfill) walks them once instead of watching.
#[derive(Debug, Parser)]
#[command(name = "phototag-watch")]
struct Cli {
    /// Path to the phototag-watch TOML config file.
    #[arg(long, default_value = "phototag-watch.toml")]
    config: PathBuf,

    /// Walk the configured roots once instead of watching.
    #[arg(long)]
    backfill: bool,

    /// With --backfill, only process the root with this name.
    #[arg(long)]
    backfill_root: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cli = Cli::parse();
    let config = config::load_config(&cli.config)?;
    let client = TaggerClient::new(config.server_url.clone());

    if cli.backfill {
        backfill::run_backfill(&config, &client, cli.backfill_root.as_deref()).await?;
    } else {
        watcher::run_watch(config, client).await?;
    }
    Ok(())
}
```

- [ ] **Step 2: Verify it builds and the CLI is wired correctly**

Run: `cargo build -p phototag-watch && ./target/debug/phototag-watch --help`
Expected: usage text listing `--config`, `--backfill`, `--backfill-root`.

- [ ] **Step 3: Verify `--backfill` runs against a real temp directory**

```bash
mkdir -p /tmp/phototag-smoketest/pictures
cat > /tmp/phototag-smoketest/config.toml <<'EOF'
server_url = "http://127.0.0.1:1"

[[roots]]
name = "pictures"
path = "/tmp/phototag-smoketest/pictures"
EOF
./target/debug/phototag-watch --config /tmp/phototag-smoketest/config.toml --backfill
rm -rf /tmp/phototag-smoketest
```
Expected: exits `0` (no files to process — the directory is empty — so it completes trivially without ever needing to reach the unreachable `server_url`).

- [ ] **Step 4: Commit**

```bash
git add crates/watch
git commit -m "Wire up phototag-watch CLI"
```

---

## Task 13: Cross-compilation for the NAS target

**Files:**
- No new files — this task documents and verifies the cross-compilation command referenced in the design spec.

`phototag-watch`'s dependencies (`tokio`, `reqwest` with `rustls`, `notify`, `clap`, `toml`, `walkdir`) are all pure-Rust or already vendor their own C code (`rustls` avoids needing system OpenSSL) — no target-specific `Cross.toml` overrides are needed, unlike `find-anything`'s (which exists solely for an unrelated Windows/mingw issue).

- [ ] **Step 1: Install `cross`, if not already installed**

Run: `cargo install cross --git https://github.com/cross-rs/cross`
Expected: installs successfully (requires Docker or Podman to actually run cross-compiled builds).

- [ ] **Step 2: Cross-compile `phototag-watch` for the NAS target**

Run: `cross build --release --target armv7-unknown-linux-gnueabihf -p phototag-watch`
Expected: `Finished` with no errors, producing `target/armv7-unknown-linux-gnueabihf/release/phototag-watch`.

If Docker/Podman isn't available in this environment, skip running this and note it as a follow-up to verify before first deployment — the command itself is what matters for the deploy scripts referenced in the design spec.

- [ ] **Step 3: Sanity-check the binary's architecture**

Run: `file target/armv7-unknown-linux-gnueabihf/release/phototag-watch`
Expected: contains `ELF 32-bit LSB ... ARM`.

- [ ] **Step 4: Commit**

Nothing to commit for this task — it's a verification step only. If you added any tooling notes, commit them:

```bash
git status
```

---

## Task 14: Example config, README, final verification

**Files:**
- Create: `phototag-watch.example.toml`
- Create: `README.md`

- [ ] **Step 1: Create the example config**

```toml
# phototag-watch.example.toml
#
# Copy this file, fill in real values, and pass it to phototag-watch with
# --config. Real deployment values (actual host, actual paths) belong in
# your own private ops config, not in this repo.

server_url = "http://phototag-server:8080"

[[roots]]
name = "pictures"
path = "/path/to/pictures"

# [[roots]]
# name = "second-library"
# path = "/path/to/other/photos"

[watch]
extensions = ["jpg", "jpeg", "png", "tiff", "heic"]
debounce_ms = 2000
```

- [ ] **Step 2: Create `README.md`**

```markdown
# phototag

Local-LLM image keyword tagging. Two binaries:

- **`phototag-server`** — stateless HTTP service. `POST /tag` with raw image
  bytes, get back `{"keywords": [...]}`. Calls a configured
  OpenAI-compatible vision model; writes nothing to disk itself.
- **`phototag-watch`** — watches configured photo library root paths (or,
  with `--backfill`, walks them once) and for any image without existing
  `IPTC:Keywords`, calls `phototag-server` and writes the returned keywords
  into the file's own `IPTC:Keywords`/`XMP-dc:Subject` metadata via
  `exiftool`.

See `AGENTS.md` for architecture and conventions, and
`docs/specs/2026-07-21-phototag-design.md` for the full design.

## Building

```bash
cargo build --release
```

`phototag-watch` also needs `exiftool` on `PATH` at runtime.

## Running `phototag-server`

```bash
PHOTOTAG_GATEWAY_URL=http://your-llm-gateway:8080/v1 \
PHOTOTAG_GATEWAY_MODEL=your-vision-model \
cargo run -p phototag-server
```

## Running `phototag-watch`

Copy `phototag-watch.example.toml`, fill in real paths and your
`phototag-server` URL, then:

```bash
cargo run -p phototag-watch -- --config your-config.toml           # watch mode
cargo run -p phototag-watch -- --config your-config.toml --backfill # one-shot catch-up
```
```

- [ ] **Step 3: Run the full workspace test suite**

Run: `cargo test --workspace`
Expected: every test across `phototag-common`, `phototag-server`, and `phototag-watch` passes.

- [ ] **Step 4: Run clippy across the workspace**

Run: `cargo clippy --workspace -- -D warnings`
Expected: no warnings. Fix anything that comes up before proceeding.

- [ ] **Step 5: Commit**

```bash
git add phototag-watch.example.toml README.md
git commit -m "Add example config, README; final verification"
```

---

## Self-review notes

- **Spec coverage:** `POST /tag` stateless service (Tasks 3–4), Dockerfile (Task 5), `exiftool`-based metadata writes with no `_original` backups (Task 6), TOML config with `[[roots]]` supporting multiple library paths (Task 7), idempotent skip-if-tagged (Tasks 9–10), `--backfill` with optional single-root restriction (Task 10), debounced watch mode following `find-watch`'s Create/Modify-plus-debounce convention rather than close-write (Task 11), cross-compilation target (Task 13). The two remaining "open items" from the spec (exact prompt text, whether `crates/common` needs a shared HTTP client) are resolved in this plan: the prompt is defined in Task 3, and `crates/common` intentionally stays limited to `TagResponse` — `phototag-server` and `phototag-watch` each use `reqwest` directly since their calling patterns don't overlap enough to justify a shared wrapper.
- **No placeholders:** every step has complete, real code — verified by re-reading through the task list above.
- **Type consistency:** `TagResponse { keywords: Vec<String> }` (Task 2) is used unchanged by `tag.rs` (Task 4) and `client.rs` (Task 8). `TagOutcome` (Task 9) is used unchanged by `backfill.rs` (Task 10) and `watcher.rs` (Task 11, via `tag_one_file`'s return type, though `watcher.rs` only logs it rather than matching on it explicitly). `WatchSettings::matches_extension` (Task 7) is used unchanged by both `backfill.rs` and `watcher.rs`, avoiding the duplicated extension-filtering logic an earlier draft of this plan had.
