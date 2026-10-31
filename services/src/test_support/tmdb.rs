//! A mock `Tmdb` and builders for the TMDB views it returns.
//!
//! A `MockTmdb` with no expectation for a method panics when that method is called, so a test
//! only sets up the calls the code under test should make. Builders return fully populated views;
//! tests override fields with struct-update syntax, e.g.
//! `MovieDetails { poster_path: None, ..movie_details(1) }`.

use std::io::Cursor;
use async_trait::async_trait;
use bytes::Bytes;
use chrono::NaiveDate;
use http::StatusCode;
use image::{ImageFormat, RgbImage};
use integrations::infrastructure::Error;
use integrations::tmdb::{ImageStream, Tmdb};
use integrations::tmdb::views::{ChangeItem, Configuration, Genre, GenreList, Languages, MovieDetails, MovieTitles, MultiMovieResult, MultiResult, MultiResults, MultiTvResult, PropertyChange, PropertyChanges, SeasonDetails, SeriesDetails, SeriesTitles, TmdbEpisode, TmdbImage, TmdbImages};

mockall::mock! {
    pub Tmdb {}

    #[async_trait]
    impl Tmdb for Tmdb {
        async fn movie_details(&self, movie_id: i32) -> Result<MovieDetails, Error>;
        async fn movie_alternative_titles(&self, movie_id: i32) -> Result<MovieTitles, Error>;
        async fn movie_changed(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<i32>, Error>;
        async fn movie_property_changes(&self, movie_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
        async fn movie_images(&self, movie_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error>;
        async fn series_details(&self, series_id: i32) -> Result<SeriesDetails, Error>;
        async fn series_alternative_titles(&self, series_id: i32) -> Result<SeriesTitles, Error>;
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
        async fn image(&self, path: &str) -> Result<Bytes, Error>;
        async fn image_stream(&self, size_slug: String, path: String)
            -> Result<(StatusCode, String, Option<ImageStream>), Error>;
    }
}

/// An integration error such as a failed TMDB request would produce, without a status code.
pub fn error(message: &str) -> Error {
    Error { message: message.into(), status_code: None, source: None }
}

/// Like `error`, but as if TMDB had answered with `status`.
pub fn error_with_status(status: StatusCode) -> Error {
    Error { message: format!("TMDB returned {}", status.as_u16()), status_code: Some(status), source: None }
}

/// A released English-language movie with backdrop and poster paths and no genres.
pub fn movie_details(id: i32) -> MovieDetails {
    MovieDetails {
        adult: Some(false),
        backdrop_path: Some(format!("/movie-{id}-backdrop.jpg")),
        id,
        imdb_id: Some(format!("tt{id:07}")),
        original_language: Some("en".into()),
        original_title: Some(format!("Movie {id}")),
        origin_country: vec!["US".into()],
        overview: Some(format!("Overview of movie {id}")),
        popularity: 1.0,
        poster_path: Some(format!("/movie-{id}-poster.jpg")),
        release_date: Some("2024-01-01".into()),
        runtime: Some(100),
        status: Some("Released".into()),
        title: format!("Movie {id}"),
        vote_average: Some(7.5),
        vote_count: Some(100),
        ..MovieDetails::default()
    }
}

/// An ended English-language series with backdrop and poster paths, no genres and no seasons.
pub fn series_details(id: i32) -> SeriesDetails {
    SeriesDetails {
        adult: Some(false),
        backdrop_path: Some(format!("/series-{id}-backdrop.jpg")),
        first_air_date: Some("2024-01-01".into()),
        id,
        in_production: Some(false),
        name: format!("Series {id}"),
        origin_country: Some(vec!["US".into()]),
        original_language: Some("en".into()),
        original_name: Some(format!("Series {id}")),
        overview: Some(format!("Overview of series {id}")),
        popularity: 1.0,
        poster_path: Some(format!("/series-{id}-poster.jpg")),
        seasons: Some(Vec::new()),
        status: Some("Ended".into()),
        r#type: Some("Scripted".into()),
        vote_average: Some(7.5),
        vote_count: Some(100),
        ..SeriesDetails::default()
    }
}

/// Season `season_number` of series `series_id` (TMDB id `series_id * 100 + season_number`),
/// with a poster path and no episodes.
pub fn season_details(series_id: i32, season_number: i32) -> SeasonDetails {
    SeasonDetails {
        id: series_id * 100 + season_number,
        air_date: Some("2024-01-01".into()),
        episodes: Vec::new(),
        name: Some(format!("Season {season_number}")),
        overview: Some(format!("Overview of season {season_number}")),
        poster_path: Some(format!("/series-{series_id}-season-{season_number}.jpg")),
        season_number: Some(season_number),
        vote_average: Some(7.5),
    }
}

/// Episode `episode_number` of season `season_number` (TMDB id
/// `season_number * 1000 + episode_number`), with a still path.
pub fn tmdb_episode(season_number: i32, episode_number: i32) -> TmdbEpisode {
    TmdbEpisode {
        id: season_number * 1000 + episode_number,
        air_date: Some("2024-01-01".into()),
        episode_number: Some(episode_number),
        name: Some(format!("Episode {episode_number}")),
        overview: Some(format!("Overview of episode {episode_number}")),
        runtime: Some(45),
        season_number: Some(season_number),
        still_path: Some(format!("/season-{season_number}-episode-{episode_number}.jpg")),
        vote_average: Some(7.5),
    }
}

/// One English backdrop (`/backdrop-1.jpg`) and one English poster (`/poster-1.jpg`), no logos.
pub fn tmdb_images() -> TmdbImages {
    fn image(file_path: &str, width: i32, height: i32) -> TmdbImage {
        TmdbImage {
            aspect_ratio: width as f32 / height as f32,
            height,
            iso_639_1: Some("en".into()),
            file_path: file_path.into(),
            vote_average: 5.0,
            vote_count: 10,
            width,
        }
    }

    TmdbImages {
        id: 1,
        backdrops: vec![image("/backdrop-1.jpg", 1920, 1080)],
        logos: Vec::new(),
        posters: vec![image("/poster-1.jpg", 1000, 1500)],
    }
}

/// A genre list from `(tmdb_id, name)` pairs.
pub fn genre_list(genres: &[(i32, &str)]) -> GenreList {
    GenreList {
        genres: genres.iter().map(|(id, name)| Genre { id: *id, name: name.to_string() }).collect(),
    }
}

/// Property changes from `(key, value)` pairs: one `updated` item per key, with no language.
/// Override `items` (e.g. `action`, `iso_639_1`) through `changes[i].items[j]`.
pub fn property_changes(changes: &[(&str, serde_json::Value)]) -> PropertyChanges {
    PropertyChanges {
        changes: changes.iter().enumerate().map(|(i, (key, value))| PropertyChange {
            key: key.to_string(),
            items: vec![ChangeItem {
                id: format!("change-{i}"),
                action: "updated".into(),
                time: "2024-01-01 00:00:00 UTC".into(),
                iso_639_1: None,
                value: Some(value.clone()),
                original_value: None,
            }],
        }).collect(),
    }
}

/// A `width` x `height` PNG, encoded in memory.
pub fn png_bytes(width: u32, height: u32) -> Bytes {
    let mut buffer = Vec::new();
    RgbImage::new(width, height)
        .write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
        .expect("encoding a test PNG");
    Bytes::from(buffer)
}

/// A movie search result with a title and a poster path, nothing else.
pub fn multi_movie(id: i32, title: &str) -> MultiResult {
    MultiResult::Movie(MultiMovieResult { id, title: Some(title.into()), poster_path: Some(format!("/poster-{id}.jpg")), ..Default::default() })
}

/// A series search result with a name and a poster path, nothing else.
pub fn multi_tv(id: i32, name: &str) -> MultiResult {
    MultiResult::Tv(MultiTvResult { id, name: Some(name.into()), poster_path: Some(format!("/poster-{id}.jpg")), ..Default::default() })
}

/// A search result page holding `results`.
pub fn multi_results(results: Vec<MultiResult>) -> MultiResults {
    MultiResults { results, ..Default::default() }
}

/// A movie search result with every field the `tmdb_search` tool reads filled in.
pub fn full_multi_movie(id: i32) -> MultiResult {
    MultiResult::Movie(MultiMovieResult {
        id,
        title: Some(format!("Movie {id}")),
        original_title: Some(format!("Original movie {id}")),
        overview: Some(format!("Overview of movie {id}")),
        release_date: Some("1979-05-25".into()),
        original_language: Some("en".into()),
        vote_average: Some(8.2),
        vote_count: Some(1000),
        ..Default::default()
    })
}

/// A series search result with every field the `tmdb_search` tool reads filled in.
pub fn full_multi_tv(id: i32) -> MultiResult {
    MultiResult::Tv(MultiTvResult {
        id,
        name: Some(format!("Series {id}")),
        original_name: Some(format!("Original series {id}")),
        overview: Some(format!("Overview of series {id}")),
        first_air_date: Some("2011-04-17".into()),
        original_language: Some("en".into()),
        vote_average: Some(8.4),
        vote_count: Some(2000),
        ..Default::default()
    })
}

/// A `Tmdb` whose `search_multi` never resolves, like a TMDB request that stalls. mockall cannot
/// express a call that never returns. Every other method panics.
pub struct UnansweringTmdb;

#[async_trait]
impl Tmdb for UnansweringTmdb {
    async fn movie_details(&self, _movie_id: i32) -> Result<MovieDetails, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn movie_alternative_titles(&self, _movie_id: i32) -> Result<MovieTitles, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn movie_changed(&self, _start: NaiveDate, _end: NaiveDate) -> Result<Vec<i32>, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn movie_property_changes(&self, _movie_id: i32, _start: NaiveDate, _end: NaiveDate) -> Result<PropertyChanges, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn movie_images(&self, _movie_id: i32, _original_language: &Option<String>) -> Result<TmdbImages, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn series_details(&self, _series_id: i32) -> Result<SeriesDetails, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn series_alternative_titles(&self, _series_id: i32) -> Result<SeriesTitles, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn series_changed(&self, _start: NaiveDate, _end: NaiveDate) -> Result<Vec<i32>, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn series_property_changes(&self, _series_id: i32, _start: NaiveDate, _end: NaiveDate) -> Result<PropertyChanges, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn season_details(&self, _series_id: i32, _season_number: i32) -> Result<SeasonDetails, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn season_property_changes(&self, _season_id: i32, _start: NaiveDate, _end: NaiveDate) -> Result<PropertyChanges, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn episode_property_changes(&self, _episode_id: i32, _start: NaiveDate, _end: NaiveDate) -> Result<PropertyChanges, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn series_images(&self, _series_id: i32, _original_language: &Option<String>) -> Result<TmdbImages, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn search_multi(&self, _query: String) -> Result<MultiResults, Error> {
        std::future::pending().await
    }
    async fn movie_genres(&self) -> Result<GenreList, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn tv_genres(&self) -> Result<GenreList, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn languages(&self) -> Result<Vec<Languages>, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn configuration(&self) -> Result<Configuration, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn image(&self, _path: &str) -> Result<Bytes, Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
    async fn image_stream(&self, _size_slug: String, _path: String) -> Result<(StatusCode, String, Option<ImageStream>), Error> {
        unimplemented!("UnansweringTmdb answers only search_multi")
    }
}
