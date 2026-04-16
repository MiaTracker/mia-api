use infrastructure::config;
use once_cell::unsync::Lazy;
use reqwest::header::{HeaderMap, HeaderValue};
use crate::infrastructure::Error;

pub const CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::new()
});

pub const TMDB_HEADERS: Lazy<HeaderMap> = Lazy::new(|| {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", HeaderValue::from_str(("Bearer ".to_owned() + config().tmdb.authorization_token.as_str()).as_str()).expect("Invalid TMDB_TOKEN header value!"));
    headers.insert("accept", HeaderValue::from_str("application/json").expect("Invalid header value!"));
    headers
});

const MAX_RETRIES: u32 = 3;

fn is_network_error(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || (e.is_request() && e.status().is_none())
}

async fn retry_get(request_fn: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, Error> {
    let mut attempt = 0u32;
    loop {
        match request_fn().send().await {
            Ok(resp) if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES => {
                let delay = resp.headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(std::time::Duration::from_secs(1 * 2u64.pow(attempt + 1)));

                tracing::warn!("Rate limited (attempt {}/{}). Retrying in {:?}.", attempt + 1, MAX_RETRIES, delay);
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Ok(resp) => return Ok(resp),
            Err(e) if attempt < MAX_RETRIES && is_network_error(&e) => {
                let delay = std::time::Duration::from_millis(250u64 * 2u64.pow(attempt));
                tracing::warn!("Request failed (attempt {}/{}): {}. Retrying in {:?}.", attempt + 1, MAX_RETRIES, e, delay);
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(e) => {
                let mut err = Error::from(e);
                if attempt > 0 {
                    err.message = format!("after {} retries: {}", attempt, err.message);
                }
                return Err(err);
            }
        }
    }
}

pub async fn tmdb_get(uri: &str) -> Result<reqwest::Response, Error> {
    retry_get(|| CLIENT.get(uri).headers(TMDB_HEADERS.clone())).await
}

pub async fn client_get(uri: &str) -> Result<reqwest::Response, Error> {
    retry_get(|| CLIENT.get(uri)).await
}
