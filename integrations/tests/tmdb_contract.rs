//! Contract tests against the real TMDB API. They need a token in `config.yaml`
//! (`tmdb.authorization_token`) and are ignored by default.
//!
//! Run with `cargo test -p integrations -- --ignored`.

use chrono::{Duration, NaiveDate, Utc};
use integrations::tmdb::views::MultiResult;
use integrations::tmdb::{Tmdb, TmdbClient};
use serde::Deserialize;

const FIGHT_CLUB: i32 = 550;
const HARRY_POTTER_1: i32 = 671;
const BREAKING_BAD: i32 = 1396;
const BREAKING_BAD_SEASON_1_ID: i32 = 3572;
const BREAKING_BAD_PILOT_ID: i32 = 62085;

#[derive(Deserialize)]
struct TestConfig {
    tmdb: TestTmdbConfig,
}

#[derive(Deserialize)]
struct TestTmdbConfig {
    authorization_token: String,
}

fn client() -> TmdbClient {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../config.yaml");
    let config: TestConfig = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_yaml::from_str(&text).ok())
        .expect("config.yaml with tmdb.authorization_token required for TMDB contract tests");
    TmdbClient::new(config.tmdb.authorization_token)
}

/// `[today - days, today]`.
fn window_of_last_days(days: i64) -> (NaiveDate, NaiveDate) {
    let today = Utc::now().date_naive();
    (today - Duration::days(days), today)
}

/// `[today - 3 days, today]`.
fn recent_window() -> (NaiveDate, NaiveDate) {
    window_of_last_days(3)
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn movie_details_returns_fight_club_when_id_is_550() {
    let movie = client().movie_details(FIGHT_CLUB).await.unwrap();

    assert_eq!(movie.id, FIGHT_CLUB);
    assert_eq!(movie.title, "Fight Club");
    assert!(movie.poster_path.is_some());
    assert!(!movie.genres.is_empty());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn movie_alternative_titles_returns_only_us_titles_when_id_is_671() {
    let titles = client().movie_alternative_titles(HARRY_POTTER_1).await.unwrap();

    assert_eq!(titles.id, HARRY_POTTER_1);
    // The request filters on country=US; Fight Club has no US titles, Harry Potter does.
    assert!(!titles.titles.is_empty());
    assert!(titles.titles.iter().all(|t| t.iso_3166_1 == "US"));
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn movie_changed_returns_more_than_one_page_of_ids_when_window_is_last_day() {
    let (start, end) = window_of_last_days(1);

    let ids = client().movie_changed(start, end).await.unwrap();

    // A page holds 100 results, so more than 100 ids proves pagination was followed.
    assert!(ids.len() > 100, "got {} ids", ids.len());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn movie_property_changes_returns_ok_when_window_is_last_three_days() {
    let (start, end) = recent_window();

    let result = client().movie_property_changes(FIGHT_CLUB, start, end).await;

    // Whether Fight Club changed in the window varies; the response shape is the contract.
    assert!(result.is_ok(), "{:?}", result.err());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn movie_images_returns_posters_and_backdrops_when_id_is_550() {
    let images = client().movie_images(FIGHT_CLUB, &Some("en".to_string())).await.unwrap();

    assert_eq!(images.id, FIGHT_CLUB);
    assert!(!images.posters.is_empty());
    assert!(!images.backdrops.is_empty());
    assert!(images.posters.iter().all(|p| p.file_path.starts_with('/')));
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn series_details_returns_breaking_bad_when_id_is_1396() {
    let series = client().series_details(BREAKING_BAD).await.unwrap();

    assert_eq!(series.id, BREAKING_BAD);
    assert_eq!(series.name, "Breaking Bad");
    assert!(!series.genres.is_empty());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn series_alternative_titles_returns_titles_when_id_is_1396() {
    let titles = client().series_alternative_titles(BREAKING_BAD).await.unwrap();

    assert_eq!(titles.id, BREAKING_BAD);
    assert!(!titles.results.is_empty());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn series_changed_returns_ids_when_window_is_last_three_days() {
    let (start, end) = recent_window();

    let ids = client().series_changed(start, end).await.unwrap();

    assert!(!ids.is_empty());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn series_property_changes_returns_ok_when_window_is_last_three_days() {
    let (start, end) = recent_window();

    let result = client().series_property_changes(BREAKING_BAD, start, end).await;

    assert!(result.is_ok(), "{:?}", result.err());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn season_details_returns_seven_episodes_when_season_is_breaking_bad_season_1() {
    let season = client().season_details(BREAKING_BAD, 1).await.unwrap();

    assert_eq!(season.id, BREAKING_BAD_SEASON_1_ID);
    assert_eq!(season.season_number, Some(1));
    assert_eq!(season.episodes.len(), 7);
    assert!(season.episodes.iter().any(|e| e.id == BREAKING_BAD_PILOT_ID));
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn season_property_changes_returns_ok_when_window_is_last_three_days() {
    let (start, end) = recent_window();

    let result = client().season_property_changes(BREAKING_BAD_SEASON_1_ID, start, end).await;

    assert!(result.is_ok(), "{:?}", result.err());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn episode_property_changes_returns_ok_when_window_is_last_three_days() {
    let (start, end) = recent_window();

    let result = client().episode_property_changes(BREAKING_BAD_PILOT_ID, start, end).await;

    assert!(result.is_ok(), "{:?}", result.err());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn series_images_returns_posters_and_backdrops_when_id_is_1396() {
    let images = client().series_images(BREAKING_BAD, &Some("en".to_string())).await.unwrap();

    assert_eq!(images.id, BREAKING_BAD);
    assert!(!images.posters.is_empty());
    assert!(!images.backdrops.is_empty());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn search_multi_returns_fight_club_when_query_is_fight_club() {
    let results = client().search_multi("Fight Club".to_string()).await.unwrap();

    assert_eq!(results.page, 1);
    assert!(results.results.iter().any(|r| matches!(
        r,
        MultiResult::Movie(m) if m.id == FIGHT_CLUB
    )));
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn movie_genres_returns_genres_when_called() {
    let genres = client().movie_genres().await.unwrap();

    assert!(!genres.genres.is_empty());
    assert!(genres.genres.iter().any(|g| g.name == "Drama"));
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn tv_genres_returns_genres_when_called() {
    let genres = client().tv_genres().await.unwrap();

    assert!(!genres.genres.is_empty());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn languages_returns_english_when_called() {
    let languages = client().languages().await.unwrap();

    assert!(languages.iter().any(|l| l.iso_639_1 == "en"));
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn configuration_returns_secure_image_base_url_when_called() {
    let configuration = client().configuration().await.unwrap();

    assert!(configuration.images.secure_base_url.starts_with("https://"));
    assert!(!configuration.images.poster_sizes.is_empty());
    assert!(!configuration.change_keys.is_empty());
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn image_returns_decodable_bytes_when_path_is_fight_club_poster() {
    let client = client();
    let poster_path = client.movie_details(FIGHT_CLUB).await.unwrap().poster_path.unwrap();

    let bytes = client.image(&poster_path).await.unwrap();

    let decoded = image::load_from_memory(&bytes).unwrap();
    assert!(decoded.width() > 0 && decoded.height() > 0);
}

#[tokio::test]
#[ignore = "requires TMDB token"]
async fn image_stream_returns_ok_image_when_size_is_w92() {
    let client = client();
    let poster_path = client.movie_details(FIGHT_CLUB).await.unwrap().poster_path.unwrap();

    let (status, content_type, stream) = client.image_stream("w92".to_string(), poster_path).await.unwrap();

    assert_eq!(status.as_u16(), 200);
    assert!(content_type.starts_with("image/"), "content type was {content_type}");
    assert!(stream.is_some());
}
