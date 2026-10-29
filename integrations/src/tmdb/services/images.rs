use futures_util::stream::{StreamExt, TryStreamExt};
use bytes::Bytes;
use reqwest::{header, Response, StatusCode};
use crate::assert_request;
use crate::infrastructure::Error;
use crate::tmdb::{ImageStream, TmdbClient};

pub(crate) async fn image(client: &TmdbClient, path: &str) -> Result<Bytes, Error> {
    let resp = image_request(client, "original", path).await?;
    assert_request!(resp);
    let bytes = resp.bytes().await?;
    Ok(bytes)
}

pub(crate) async fn image_stream(client: &TmdbClient, size_slug: String, path: String) -> Result<(StatusCode, String, Option<ImageStream>), Error> {
    let resp = image_request(client, size_slug.as_str(), path.as_str()).await?;
    let status_code = resp.status();
    let content_type = resp.headers().get(header::CONTENT_TYPE)
        .map_or("image/webp", |v| v.to_str().unwrap_or("image/webp"))
        .to_string();
    if status_code != StatusCode::OK {
        return Ok((status_code, content_type, None));
    }
    let stream = resp.bytes_stream().map_err(|e| e.into()).boxed();
    Ok((status_code, content_type, Some(stream)))
}

async fn image_request(client: &TmdbClient, size_slug: &str, path: &str) -> Result<Response, Error> {
    let conf = &client.cached_configuration().await?.images;
    let uri = format!("{}{}{}", conf.secure_base_url.as_str(), size_slug, path);
    client.client_get(&uri).await
}
