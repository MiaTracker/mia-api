use async_trait::async_trait;
use bytes::Bytes;
use chrono::NaiveDate;
use futures_util::stream::BoxStream;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Response, StatusCode};
use tokio::sync::OnceCell;
use crate::constants;
use crate::infrastructure::{retry_get, Error};
use crate::tmdb::services::{configuration, genres, images, movies, search, series};
use crate::tmdb::views::{Configuration, GenreList, Languages, MovieDetails, MovieTitles, MultiResults, PropertyChanges, SeasonDetails, SeriesDetails, SeriesTitles, TmdbImages};

/// The body of an image streamed from TMDB.
pub type ImageStream = BoxStream<'static, Result<Bytes, Error>>;

/// Every TMDB operation the application uses.
#[async_trait]
pub trait Tmdb: Send + Sync {
    async fn movie_details(&self, movie_id: i32) -> Result<MovieDetails, Error>;
    async fn movie_alternative_titles(&self, movie_id: i32) -> Result<MovieTitles, Error>;
    /// Ids of all movies changed between `start` and `end`, across all result pages.
    async fn movie_changed(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<i32>, Error>;
    async fn movie_property_changes(&self, movie_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn movie_images(&self, movie_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error>;
    async fn series_details(&self, series_id: i32) -> Result<SeriesDetails, Error>;
    async fn series_alternative_titles(&self, series_id: i32) -> Result<SeriesTitles, Error>;
    /// Ids of all series changed between `start` and `end`, across all result pages.
    async fn series_changed(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<i32>, Error>;
    async fn series_property_changes(&self, series_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn season_details(&self, series_id: i32, season_number: i32) -> Result<SeasonDetails, Error>;
    async fn season_property_changes(&self, season_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn episode_property_changes(&self, episode_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn series_images(&self, series_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error>;
    async fn search_multi(&self, query: String) -> Result<MultiResults, Error>;
    async fn movie_genres(&self) -> Result<GenreList, Error>;
    async fn tv_genres(&self) -> Result<GenreList, Error>;
    async fn languages(&self) -> Result<Vec<Languages>, Error>;
    async fn configuration(&self) -> Result<Configuration, Error>;
    /// The original-size image at `path`. Fails unless TMDB answers 200.
    async fn image(&self, path: &str) -> Result<Bytes, Error>;
    /// Streams the image at `path` in size `size_slug`. A non-200 answer is returned as its
    /// status and content type, without a body.
    async fn image_stream(&self, size_slug: String, path: String)
        -> Result<(StatusCode, String, Option<ImageStream>), Error>;
}

/// The real TMDB API client.
///
/// Construction makes no network calls. The TMDB configuration (which holds the image base URL)
/// is fetched on first use and cached for the lifetime of the client.
pub struct TmdbClient {
    http: reqwest::Client,
    authorization_token: String,
    base_url: String,
    configuration: OnceCell<Configuration>,
}

impl TmdbClient {
    pub fn new(authorization_token: String) -> Self {
        Self::with_base_url(authorization_token, constants::TMDB_URL.to_string())
    }

    /// Like `new`, but against another API root; `base_url` must end with `/`.
    pub fn with_base_url(authorization_token: String, base_url: String) -> Self {
        Self {
            http: reqwest::Client::new(),
            authorization_token,
            base_url,
            configuration: OnceCell::new(),
        }
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    fn tmdb_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", HeaderValue::from_str(("Bearer ".to_owned() + self.authorization_token.as_str()).as_str()).expect("Invalid TMDB_TOKEN header value!"));
        headers.insert("accept", HeaderValue::from_str("application/json").expect("Invalid header value!"));
        headers
    }

    /// GET against the TMDB API, with authorization.
    pub(crate) async fn tmdb_get(&self, uri: &str) -> Result<Response, Error> {
        retry_get(|| self.http.get(uri).headers(self.tmdb_headers())).await
    }

    /// GET against any URL (e.g. the image CDN), without authorization.
    pub(crate) async fn client_get(&self, uri: &str) -> Result<Response, Error> {
        retry_get(|| self.http.get(uri)).await
    }

    pub(crate) async fn cached_configuration(&self) -> Result<&Configuration, Error> {
        self.configuration.get_or_try_init(|| configuration::details(self)).await
    }
}

#[async_trait]
impl Tmdb for TmdbClient {
    async fn movie_details(&self, movie_id: i32) -> Result<MovieDetails, Error> {
        movies::details(self, movie_id).await
    }

    async fn movie_alternative_titles(&self, movie_id: i32) -> Result<MovieTitles, Error> {
        movies::alternative_titles(self, movie_id).await
    }

    async fn movie_changed(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<i32>, Error> {
        movies::changed(self, start, end).await
    }

    async fn movie_property_changes(&self, movie_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error> {
        movies::property_changes(self, movie_id, start, end).await
    }

    async fn movie_images(&self, movie_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error> {
        movies::images(self, movie_id, original_language).await
    }

    async fn series_details(&self, series_id: i32) -> Result<SeriesDetails, Error> {
        series::details(self, series_id).await
    }

    async fn series_alternative_titles(&self, series_id: i32) -> Result<SeriesTitles, Error> {
        series::alternative_titles(self, series_id).await
    }

    async fn series_changed(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<i32>, Error> {
        series::changed(self, start, end).await
    }

    async fn series_property_changes(&self, series_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error> {
        series::property_changes(self, series_id, start, end).await
    }

    async fn season_details(&self, series_id: i32, season_number: i32) -> Result<SeasonDetails, Error> {
        series::season_details(self, series_id, season_number).await
    }

    async fn season_property_changes(&self, season_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error> {
        series::season_property_changes(self, season_id, start, end).await
    }

    async fn episode_property_changes(&self, episode_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error> {
        series::episode_property_changes(self, episode_id, start, end).await
    }

    async fn series_images(&self, series_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error> {
        series::images(self, series_id, original_language).await
    }

    async fn search_multi(&self, query: String) -> Result<MultiResults, Error> {
        search::multi(self, query).await
    }

    async fn movie_genres(&self) -> Result<GenreList, Error> {
        genres::movie(self).await
    }

    async fn tv_genres(&self) -> Result<GenreList, Error> {
        genres::tv(self).await
    }

    async fn languages(&self) -> Result<Vec<Languages>, Error> {
        configuration::languages(self).await
    }

    /// Fetched once per client, then served from the cache.
    async fn configuration(&self) -> Result<Configuration, Error> {
        self.cached_configuration().await.cloned()
    }

    async fn image(&self, path: &str) -> Result<Bytes, Error> {
        images::image(self, path).await
    }

    async fn image_stream(&self, size_slug: String, path: String)
        -> Result<(StatusCode, String, Option<ImageStream>), Error> {
        images::image_stream(self, size_slug, path).await
    }
}
