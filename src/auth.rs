use std::path::PathBuf;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
    basic::BasicClient,
};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type ConfiguredClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Debug, Deserialize)]
struct GoogleCredentials {
    installed: GoogleInstalled,
}

#[derive(Debug, Deserialize)]
struct GoogleInstalled {
    client_id: String,
    client_secret: String,
    auth_uri: String,
    token_uri: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StoredToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

pub struct GoogleAuth {
    client: ConfiguredClient,
    token_path: PathBuf,
    token: Option<StoredToken>,
}

impl GoogleAuth {
    pub async fn load(credentials_path: &PathBuf, token_path: PathBuf) -> Result<Self> {
        if !credentials_path.exists() {
            anyhow::bail!(
                "credentials.json not found at {}",
                credentials_path.display()
            );
        }

        let creds_json = std::fs::read_to_string(credentials_path)
            .context("Failed to read credentials.json")?;
        let creds: GoogleCredentials =
            serde_json::from_str(&creds_json).context("Failed to parse credentials.json")?;

        let redirect_uri = "http://localhost:8080".to_string();

        let client = BasicClient::new(ClientId::new(creds.installed.client_id))
            .set_client_secret(ClientSecret::new(creds.installed.client_secret))
            .set_auth_uri(AuthUrl::new(creds.installed.auth_uri)?)
            .set_token_uri(TokenUrl::new(creds.installed.token_uri)?)
            .set_redirect_uri(RedirectUrl::new(redirect_uri)?);

        let token = if token_path.exists() {
            let raw = std::fs::read_to_string(&token_path)?;
            serde_json::from_str(&raw).ok()
        } else {
            None
        };

        Ok(GoogleAuth {
            client,
            token_path,
            token,
        })
    }

    pub fn needs_auth(&self) -> bool {
        self.token.is_none()
    }

    /// Generate the authorization URL and CSRF token.
    pub fn generate_auth_url(&self) -> (String, String) {
        let (url, csrf) = self
            .client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/calendar".to_string(),
            ))
            .url();
        (url.to_string(), csrf.secret().clone())
    }

    /// Full auth flow: opens browser, listens for redirect, exchanges code.
    pub async fn authenticate(&mut self) -> Result<()> {
        let (url, csrf) = self.generate_auth_url();

        println!("Opening browser for Google Calendar authorization...");
        if std::process::Command::new("open").arg(&url).spawn().is_err() {
            println!("Please open this URL in your browser:\n{}", url);
        }

        let (code, state) = listen_for_redirect(8080).await?;
        if state != csrf {
            anyhow::bail!("CSRF state mismatch");
        }

        let response = self.exchange_code_raw(&code).await?;
        self.store_token(&response)?;
        println!("Authentication successful!");
        Ok(())
    }

    /// Exchange authorization code without CSRF check (already verified by caller).
    pub async fn exchange_code_raw(&self, code: &str) -> Result<oauth2::basic::BasicTokenResponse> {
        let http_client = reqwest::Client::new();
        let token_response = self
            .client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .request_async(&http_client)
            .await
            .context("Failed to exchange authorization code for token")?;
        Ok(token_response)
    }

    // ── Token helpers ────────────────────────────────────────

    pub fn store_token(&self, token_response: &oauth2::basic::BasicTokenResponse) -> Result<()> {
        let expires_at = token_response.expires_in().map(|dur| {
            let secs = dur.as_secs() as i64;
            Utc::now() + chrono::Duration::seconds(secs)
        });

        let stored = StoredToken {
            access_token: token_response.access_token().secret().clone(),
            refresh_token: token_response.refresh_token().map(|t| t.secret().clone()),
            expires_at,
        };

        let json = serde_json::to_string_pretty(&stored)?;
        std::fs::write(&self.token_path, json)?;
        Ok(())
    }

    pub async fn get_access_token(&mut self) -> Result<String> {
        if let Some(ref token) = self.token {
            if let Some(expires_at) = token.expires_at
                && Utc::now() < expires_at {
                    return Ok(token.access_token.clone());
                }

            if let Some(ref refresh_token_str) = token.refresh_token {
                let http_client = reqwest::Client::new();
                let new_token = self
                    .client
                    .exchange_refresh_token(&RefreshToken::new(refresh_token_str.clone()))
                    .request_async(&http_client)
                    .await
                    .context("Failed to refresh token")?;

                let stored = StoredToken {
                    access_token: new_token.access_token().secret().clone(),
                    refresh_token: new_token.refresh_token().map(|t| t.secret().clone()),
                    expires_at: None,
                };
                let json = serde_json::to_string_pretty(&stored)?;
                std::fs::write(&self.token_path, json)?;
                self.token = Some(stored);

                return Ok(new_token.access_token().secret().clone());
            }
        }

        self.authenticate().await?;
        match self.token {
            Some(ref token) => Ok(token.access_token.clone()),
            None => anyhow::bail!("Authentication failed"),
        }
    }
}

/// Bind a TCP listener on localhost and wait for the OAuth redirect.
async fn listen_for_redirect(port: u16) -> Result<(String, String)> {
    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr)
        .await
        .with_context(|| format!("Failed to bind to {}. Port may be in use.", addr))?;

    let (mut stream, _) = listener
        .accept()
        .await
        .context("Failed to accept OAuth redirect connection")?;

    let mut buf = vec![0; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .context("Failed to read OAuth redirect")?;
    let request = String::from_utf8_lossy(&buf[..n]);

    let (code, state) = parse_redirect(&request)?;

    let response = "\
        HTTP/1.1 200 OK\r\n\
        Content-Type: text/html; charset=utf-8\r\n\
        Content-Length: 101\r\n\r\n\
        <html><body><h1>Authorization successful!</h1>\
        <p>You can close this window and return to the terminal.</p></body></html>";
    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;

    Ok((code, state))
}

pub fn parse_redirect(request: &str) -> Result<(String, String)> {
    let first_line = request.lines().next().context("Empty HTTP request")?;
    let path = first_line
        .split_whitespace()
        .nth(1)
        .context("Malformed request line")?;

    let parsed_url =
        url::Url::parse(&format!("http://localhost{}", path)).context("Failed to parse URL")?;

    let code = parsed_url
        .query_pairs()
        .find(|(k, _)| k == "code")
        .map(|(_, v)| v.to_string())
        .context("No authorization code in redirect")?;

    let state = parsed_url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .map(|(_, v)| v.to_string())
        .context("No state in redirect")?;

    Ok((code, state))
}
