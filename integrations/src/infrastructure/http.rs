use std::env;
use once_cell::unsync::Lazy;
use reqwest::header::{HeaderMap, HeaderValue};

pub const CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::new()
});

pub const TMDB_HEADERS: Lazy<HeaderMap> = Lazy::new(|| {
    let mut headers = HeaderMap::new();
    headers.insert("Authorization", HeaderValue::from_str(("Bearer ".to_owned() + TMDB_TOKEN.as_str()).as_str()).expect("Invalid TMDB_TOKEN header value!"));
    headers.insert("accept", HeaderValue::from_str("application/json").expect("Invalid header value!"));
    headers
});

pub const TMDB_TOKEN: Lazy<String> = Lazy::new(|| {
    env::var("TMDB_AUTHORIZATION_TOKEN").expect("TMDB_TOKEN env var not defined!")
});