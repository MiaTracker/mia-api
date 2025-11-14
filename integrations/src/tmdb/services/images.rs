use futures_util::stream::TryStreamExt;
use bytes::Bytes;
use reqwest::{header, Response, StatusCode};
use crate::{assert_request, constants};
use crate::infrastructure::{Error, CLIENT, TMDB_HEADERS};

pub async fn image(path: &str) -> Result<Bytes, Error> {
    let resp = image_request("original", path).await?;
    assert_request!(resp);
    let bytes = resp.bytes().await?;
    Ok(bytes)
}

pub async fn image_stream(size_slug: String, path: String) -> Result<(StatusCode, String, Option<impl futures_core::Stream<Item = Result<Bytes, Error>>>), Error> {
    let resp = image_request(size_slug.as_str(), path.as_str()).await?;
    let status_code = resp.status();
    let content_type = resp.headers().get(header::CONTENT_TYPE)
        .map_or("image/webp", |v| v.to_str().unwrap_or("image/webp"))
        .to_string();
    if status_code != StatusCode::OK {
        return Ok((status_code, content_type, None));
    }
    let stream = resp.bytes_stream().map_err(|e| e.into());
    Ok((status_code, content_type, Some(stream)))
}

async fn image_request(size_slug: &str, path: &str) -> Result<Response, Error> {
    let conf = &constants::TMDB_CONFIGURATION.get().expect("Failed to get TMDB_CONFIGURATION").images;
    let uri = format!("{}{}{}", conf.secure_base_url.as_str(), size_slug, path);
    let resp = CLIENT.get(uri).headers(TMDB_HEADERS.clone()).send().await?;
    Ok(resp)
}