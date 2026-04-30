//! `intr login` — authenticate and store an API key.
//!
//! ## Flow
//!
//! 1. Spin up a local callback server on a random port.
//! 2. Open the browser to `https://intentry.dev/cli-auth?callback_port=PORT&state=NONCE`.
//!    The `/cli-auth` web page authenticates via Clerk, creates an API key, then
//!    redirects to `http://localhost:PORT/token?key=<intr_live_...>&state=NONCE`.
//! 3. CLI receives the token, validates it with `GET /v1/me`, stores in keychain.
//!
//! ## Fallback (while /cli-auth page is being built)
//!
//! If the browser callback does not arrive within 30 s, the CLI falls back to
//! prompting the user to paste a token manually.  Tokens can be created at
//! `https://intentry.dev/settings/tokens`.
//!
//! ## Direct token flag
//!
//! `intr login --token <intr_live_...>` skips the browser flow entirely.
//! Useful for CI/CD and headless environments.

use std::{
    net::{SocketAddr, TcpListener},
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::{
    auth,
    client::IntrClient,
    config::Config,
    error::{CliError, CliResult},
    ui::output,
};

pub async fn run(json: bool) -> CliResult<()> {
    run_with_token(None, json).await
}

pub async fn run_with_token(explicit_token: Option<&str>, json: bool) -> CliResult<()> {
    let config = Config::load()?;

    if let Some(token) = explicit_token {
        return validate_and_store(token, &config, json).await;
    }

    // ------------------------------------------------------------------
    // Step 1: find a free local port
    // ------------------------------------------------------------------
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| CliError::Generic(format!("could not bind local port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| CliError::Generic(e.to_string()))?
        .port();
    drop(listener); // release so axum can bind it

    // ------------------------------------------------------------------
    // Step 2: generate a state nonce to prevent CSRF
    // ------------------------------------------------------------------
    let nonce: String = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut h);
        port.hash(&mut h);
        format!("{:016x}", h.finish())
    };

    // ------------------------------------------------------------------
    // Step 3: start the local callback server
    // ------------------------------------------------------------------
    let received_token: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let token_ref = received_token.clone();
    let nonce_ref = nonce.clone();

    let server = tokio::spawn(async move {
        run_callback_server(port, nonce_ref, token_ref).await
    });

    // ------------------------------------------------------------------
    // Step 4: open the browser
    // ------------------------------------------------------------------
    let base_web = config
        .auth
        .api_base_url
        .as_deref()
        .unwrap_or("https://api.intentry.dev")
        .replace("api.intentry.dev", "intentry.dev")
        .replace("https://intentry.dev/v1", "https://intentry.dev");
    let auth_url = format!("{base_web}/cli-auth?callback_port={port}&state={nonce}");

    output::print_info(&format!(
        "Opening browser to authenticate…\n\n  {auth_url}\n"
    ));
    output::print_info("Waiting for authentication (30 s timeout)…");

    if let Err(e) = open::that(&auth_url) {
        output::print_warn(&format!(
            "could not open browser automatically: {e}\n  Open the URL above manually."
        ));
    }

    // ------------------------------------------------------------------
    // Step 5: wait up to 30 s for the browser callback
    // ------------------------------------------------------------------
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let token = loop {
        if tokio::time::Instant::now() >= deadline {
            break None;
        }
        {
            if let Ok(guard) = received_token.lock() {
                if guard.is_some() {
                    break guard.clone();
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    server.abort();

    // ------------------------------------------------------------------
    // Step 6: use received token OR fall back to manual paste
    // ------------------------------------------------------------------
    let token = if let Some(t) = token {
        t
    } else {
        output::print_warn("Browser callback not received. Falling back to manual token entry.");
        output::print_info(&format!(
            "\nCreate an API key at: {base_web}/settings/tokens\n"
        ));
        prompt_token()?
    };

    validate_and_store(&token, &config, json).await
}

/// Validate a token by calling GET /v1/me, then store it in the keychain.
async fn validate_and_store(token: &str, config: &Config, json: bool) -> CliResult<()> {
    let client = IntrClient::new(config, token.to_owned());
    let me = client.get_me().await.map_err(|e| match e {
        CliError::Auth(_) => CliError::Auth("invalid API key — please check and try again".to_string()),
        other => other,
    })?;

    auth::store_token(token)?;

    if json {
        output::print_json_ok(&serde_json::json!({
            "logged_in": true,
            "username": me.username,
            "email": me.email,
        }));
    } else {
        output::print_success(&format!(
            "Logged in as {} ({})",
            me.username, me.email
        ));
    }
    Ok(())
}

/// Prompt the user to paste a token interactively.
fn prompt_token() -> CliResult<String> {
    use dialoguer::Password;
    Password::new()
        .with_prompt("Paste your API key")
        .interact()
        .map_err(|e| CliError::Generic(format!("input error: {e}")))
}

/// Minimal HTTP server that listens for the browser callback on localhost.
///
/// Expected request: `GET /token?key=<intr_live_...>&state=<nonce>`
async fn run_callback_server(
    port: u16,
    expected_nonce: String,
    result: Arc<Mutex<Option<String>>>,
) {
    use std::collections::HashMap;

    let addr: SocketAddr = ([127, 0, 0, 1], port).into();
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(_) => return,
    };

    // Accept a single connection then exit.
    if let Ok((mut stream, _)) = listener.accept().await {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let mut reader = BufReader::new(&mut stream);
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).await.is_err() {
            return;
        }

        // Parse "GET /token?key=...&state=... HTTP/1.1"
        if let Some(path) = request_line
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.strip_prefix("/token?"))
        {
            let params: HashMap<&str, &str> = path
                .split('&')
                .filter_map(|pair| {
                    let mut parts = pair.splitn(2, '=');
                    Some((parts.next()?, parts.next()?))
                })
                .collect();

            if params.get("state").copied() == Some(expected_nonce.as_str()) {
                if let Some(key) = params.get("key").copied() {
                    if !key.is_empty() {
                        if let Ok(mut guard) = result.lock() {
                            *guard = Some(key.to_owned());
                        }
                    }
                }
            }
        }

        // Respond with a success page then close.
        let body = b"<html><body><h2>Authenticated!</h2><p>You can close this tab and return to the terminal.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.write_all(body).await;
    }
}

