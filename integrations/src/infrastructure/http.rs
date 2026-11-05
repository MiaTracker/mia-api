use crate::infrastructure::Error;

const MAX_RETRIES: u32 = 5;

fn is_network_error(e: &reqwest::Error) -> bool {
    e.is_connect() || e.is_timeout() || (e.is_request() && e.status().is_none())
}

/// Sends the request built by `request_fn`, rebuilding and retrying it on rate limiting and
/// network errors.
pub(crate) async fn retry_get(request_fn: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, Error> {
    let mut attempt = 0u32;
    loop {
        match request_fn().send().await {
            Ok(resp) if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES => {
                let delay = resp.headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(std::time::Duration::from_secs)
                    .unwrap_or(std::time::Duration::from_secs(2u64.pow(attempt + 1)));

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
