//! HTTP client for api.intentry.dev.
//!
//! Wraps `reqwest` with:
//! - `Authorization: Bearer <api_key>` on every authenticated request
//! - Typed request/response structs mirroring the cloud API
//! - Consistent error mapping into `CliError`

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{
    config::Config,
    error::{CliError, CliResult},
};

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

pub struct IntrClient {
    base: String,
    client: Client,
    api_key: Option<String>,
}

impl IntrClient {
    /// Authenticated client (most commands).
    pub fn new(config: &Config, api_key: String) -> Self {
        Self {
            base: config.api_base_url().trim_end_matches('/').to_owned(),
            client: build_client(),
            api_key: Some(api_key),
        }
    }

    /// Unauthenticated client — for public endpoints (search, commons).
    pub fn anonymous(config: &Config) -> Self {
        Self {
            base: config.api_base_url().trim_end_matches('/').to_owned(),
            client: build_client(),
            api_key: None,
        }
    }

    fn auth_header(&self) -> Option<String> {
        self.api_key.as_ref().map(|k| format!("Bearer {k}"))
    }

    // -- Low-level helpers -------------------------------------------------

    async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> CliResult<T> {
        let url = format!("{}{path}", self.base);
        let mut req = self.client.get(&url);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        handle_response(req.send().await.map_err(net_err)?).await
    }

    async fn post_json<B: Serialize, T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        body: &B,
    ) -> CliResult<T> {
        let url = format!("{}{path}", self.base);
        let mut req = self.client.post(&url).json(body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        handle_response(req.send().await.map_err(net_err)?).await
    }

    /// POST that expects a 2xx with no body (e.g. 201 Created).
    async fn post_no_body<B: Serialize>(&self, path: &str, body: &B) -> CliResult<()> {
        let url = format!("{}{path}", self.base);
        let mut req = self.client.post(&url).json(body);
        if let Some(auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }
        let resp = req.send().await.map_err(net_err)?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(map_status(status, &text))
        }
    }

    // -- API methods -------------------------------------------------------

    /// `GET /v1/me`
    pub async fn get_me(&self) -> CliResult<MeResponse> {
        self.get("/v1/me").await
    }

    /// `POST /v1/me/tokens` — create a long-lived API key.
    ///
    /// Requires a valid Clerk JWT (or existing API key) in `Authorization`.
    pub async fn create_token(&self, name: &str) -> CliResult<ApiTokenResponse> {
        self.post_json("/v1/me/tokens", &CreateTokenRequest { name: name.to_owned() })
            .await
    }

    /// `POST /v1/events/batch` — push local events to the remote.
    pub async fn push_events(&self, events: Vec<PushEventItem>) -> CliResult<()> {
        self.post_no_body("/v1/events/batch", &BatchPushRequest { events })
            .await
    }

    /// `GET /v1/events` — pull remote events since `cursor`.
    pub async fn pull_events(&self, cursor: Option<&str>) -> CliResult<PaginatedEvents> {
        let path = match cursor {
            Some(c) => format!("/v1/events?cursor={c}&limit=100"),
            None => "/v1/events?limit=100".to_string(),
        };
        self.get(&path).await
    }

    /// `GET /v1/search`
    pub async fn search(&self, query: &str, limit: u32) -> CliResult<SearchResponse> {
        // Build query params manually to avoid adding a URL encoding crate.
        let q: String = query
            .bytes()
            .flat_map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    vec![b as char]
                }
                b' ' => vec!['+'],
                _ => format!("%{b:02X}").chars().collect(),
            })
            .collect();
        self.get(&format!("/v1/search?q={q}&limit={limit}")).await
    }
}

fn build_client() -> Client {
    Client::builder()
        .user_agent(format!("intr-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build HTTP client")
}

fn net_err(e: reqwest::Error) -> CliError {
    CliError::Network(e.to_string())
}

fn map_status(status: StatusCode, body: &str) -> CliError {
    match status {
        StatusCode::UNAUTHORIZED => {
            CliError::Auth("authentication failed — run `intr login` again".to_string())
        }
        StatusCode::FORBIDDEN => CliError::Auth("permission denied".to_string()),
        StatusCode::NOT_FOUND => CliError::Generic("not found".to_string()),
        s => CliError::Network(format!("API error {s}: {body}")),
    }
}

async fn handle_response<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
) -> CliResult<T> {
    if resp.status().is_success() {
        resp.json::<T>()
            .await
            .map_err(|e| CliError::Network(format!("failed to decode response: {e}")))
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        Err(map_status(status, &body))
    }
}

// ---------------------------------------------------------------------------
// Request / response types (mirror the cloud API shapes)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MeResponse {
    pub id: String,
    pub email: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateTokenRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct ApiTokenResponse {
    pub id: String,
    pub name: String,
    /// Only present in the creation response.
    pub token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PushEventItem {
    pub event_type: String,
    pub space_id: Option<String>,
    pub prompt_id: Option<String>,
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct BatchPushRequest {
    pub events: Vec<PushEventItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EventResponse {
    pub id: String,
    pub space_id: Option<String>,
    pub prompt_id: Option<String>,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct PaginatedEvents {
    pub items: Vec<EventResponse>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub id: String,
    pub space_slug: String,
    pub slug: String,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub current_version: String,
}

#[derive(Debug, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<SearchResultItem>,
    pub query: String,
    pub total: usize,
}
