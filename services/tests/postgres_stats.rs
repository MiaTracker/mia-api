//! Statistics against a real Postgres (grouped, joined and aggregated SQL). Needs Docker:
//! `cargo test -p services --test postgres_stats -- --ignored`.

mod common;

use common::postgres::database;
use common::seed;
use entities::sea_orm_active_enums::{ImageFileType, ImageSource, ImageType, MediaType};
use entities::{image_sizes, images, media};
use sea_orm::{ActiveModelTrait, DbConn, NotSet, Set};
use services::statistics;
use views::media::MediaIndex;

async fn rate(db: &DbConn, media_id: i32, stars: Option<f32>, language: Option<&str>) {
    seed::update_media(db, media_id, media::ActiveModel {
        stars: Set(stars),
        original_language: Set(language.map(str::to_string)),
        ..Default::default()
    }).await;
}

/// Gives the media a 300x450 poster with one size.
async fn poster(db: &DbConn, media_id: i32) {
    let image = images::ActiveModel {
        id: NotSet,
        path: Set("0123456789abcdef0123456789abcdef".into()),
        source: Set(ImageSource::Tmdb),
        source_path: Set(Some("/poster.jpg".into())),
        r#type: Set(ImageType::Poster),
        file_type: Set(ImageFileType::Jpeg),
    }.insert(db).await.expect("inserting image");
    image_sizes::ActiveModel { id: NotSet, image_id: Set(image.id), width: Set(300), height: Set(450) }
        .insert(db).await.expect("inserting image size");
    seed::update_media(db, media_id, media::ActiveModel { poster_image_id: Set(Some(image.id)), ..Default::default() }).await;
}

fn title(index: &Option<MediaIndex>) -> Option<&str> {
    index.as_ref().map(|index| index.title.as_str())
}

/// | title  | type   | stars | language | genre  | logs (completed) |
/// |--------|--------|-------|----------|--------|------------------|
/// | Matrix | movie  | 8.5   | en       | Action | 2 (2)            |
/// | Alien  | movie  | 9     | none     | Horror | 1 (1)            |
/// | Dark   | series | none  | de       | Drama  | 2 (0)            |
/// | Lost   | series | 7     | en       | Drama  | 1 (1)            |
///
/// Another user's movie with a log must not count. The most watched and the highest rated media
/// differ within each type (when they coincide, `stats` has a known bug).
#[tokio::test]
#[ignore = "requires docker"]
async fn stats_runs_against_real_schema() {
    let db = database().await;
    seed::language(&db, "en", "English").await;
    seed::language(&db, "de", "German").await;
    let user = seed::user(&db, 1).await;

    let matrix = seed::movie(&db, &user, 603, "Matrix").await;
    rate(&db, matrix.id, Some(8.5), Some("en")).await;
    poster(&db, matrix.id).await;
    seed::genre(&db, matrix.id, "Action", MediaType::Movie, Some(28)).await;
    let source = seed::source(&db, matrix.id, "Jellyfin").await;
    seed::log(&db, matrix.id, source.id, Some(8.0), true).await;
    seed::log(&db, matrix.id, source.id, Some(9.0), true).await;

    let alien = seed::movie(&db, &user, 348, "Alien").await;
    rate(&db, alien.id, Some(9.0), None).await;
    seed::genre(&db, alien.id, "Horror", MediaType::Movie, Some(27)).await;
    let source = seed::source(&db, alien.id, "Jellyfin").await;
    seed::log(&db, alien.id, source.id, Some(9.0), true).await;

    let dark = seed::series(&db, &user, 70523, "Dark").await;
    rate(&db, dark.id, None, Some("de")).await;
    let drama = seed::genre(&db, dark.id, "Drama", MediaType::Series, Some(18)).await;
    let source = seed::source(&db, dark.id, "Netflix").await;
    seed::log(&db, dark.id, source.id, None, false).await;
    seed::log(&db, dark.id, source.id, None, false).await;

    let lost = seed::series(&db, &user, 4607, "Lost").await;
    rate(&db, lost.id, Some(7.0), Some("en")).await;
    entities::media_genres::ActiveModel { media_id: Set(lost.id), genre_id: Set(drama.id) }
        .insert(&db).await.expect("inserting media genre");
    let source = seed::source(&db, lost.id, "Netflix").await;
    seed::log(&db, lost.id, source.id, Some(7.0), true).await;

    let other = seed::user(&db, 2).await;
    let others = seed::movie(&db, &other, 604, "Other").await;
    rate(&db, others.id, Some(10.0), Some("en")).await;
    let source = seed::source(&db, others.id, "Jellyfin").await;
    seed::log(&db, others.id, source.id, Some(10.0), true).await;

    let stats = statistics::stats(&user, &db).await.ok().expect("stats should succeed");

    assert_eq!((stats.media.count, stats.media.movies, stats.media.series), (4, 2, 2));
    assert_eq!((stats.logs.logs, stats.logs.completed, stats.logs.uncompleted), (6, 4, 2));

    let mut genres: Vec<(String, i64)> = stats.genres.into_iter().map(|g| (g.name, g.count)).collect();
    genres.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    assert_eq!(genres, vec![("Drama".to_string(), 2), ("Action".to_string(), 1), ("Horror".to_string(), 1)]);
    let mut languages: Vec<(String, i64)> = stats.languages.into_iter().map(|l| (l.name, l.count)).collect();
    languages.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    assert_eq!(languages, vec![("English".to_string(), 2), ("German".to_string(), 1)]);

    assert_eq!((title(&stats.most_watched.movie), title(&stats.most_watched.series)), (Some("Matrix"), Some("Dark")));
    assert_eq!((title(&stats.highest_rated.movie), title(&stats.highest_rated.series)), (Some("Alien"), Some("Lost")));
    let matrix_poster = stats.most_watched.movie.and_then(|index| index.poster).expect("the most watched movie has a poster");
    let sizes: Vec<(i32, i32)> = matrix_poster.sizes.iter().map(|s| (s.width, s.height)).collect();
    assert_eq!(sizes, vec![(300, 450)]);

    let average = stats.average_rating;
    let close = |actual: Option<f64>, expected: f64| actual.is_some_and(|a| (a - expected).abs() < 1e-4);
    assert!(close(average.overall, (8.5 + 9.0 + 7.0) / 3.0), "overall average {:?}", average.overall);
    assert!(close(average.movies, 8.75), "movie average {:?}", average.movies);
    assert!(close(average.series, 7.0), "series average {:?}", average.series);
}
