//! Movie and series creation and media deletion against a real Postgres, with images saved
//! through `InlineJobs` into an in-memory store. Needs Docker:
//! `cargo test -p services --test postgres_media -- --ignored`.

mod common;

use std::sync::Arc;
use bytes::Bytes;
use common::postgres::database;
use common::{fakes, seed};
use entities::prelude::{Episodes, Genres, ImageSizes, Images, Media, MediaGenres, Movies, Seasons, Series, Tags, Titles};
use entities::sea_orm_active_enums::{ImageType, MediaType};
use entities::{episodes, genres, image_sizes, images, seasons, titles};
use integrations::tmdb::views::{Genre, MovieDetails, MovieTitle, MovieTitles, Season, SeasonDetails, SeriesDetails, SeriesTitles};
use sea_orm::{ColumnTrait, DbConn, EntityTrait, ModelTrait, QueryFilter, QueryOrder};
use services::test_support::tmdb::{movie_details, png_bytes, season_details, series_details, tmdb_episode};
use services::test_support::{date, MockTmdb};
use services::{media, movies, series};
use views::media::MediaType as ViewMediaType;

/// A `MockTmdb` for `movies::create`: `movie_details` from `details`, one German alternative
/// title, and `image` bytes for every image path.
fn movie_tmdb(details: fn(i32) -> MovieDetails, image: Bytes) -> MockTmdb {
    let mut tmdb = MockTmdb::new();
    tmdb.expect_movie_details().returning(move |id| Ok(details(id)));
    tmdb.expect_movie_alternative_titles().returning(|id| Ok(MovieTitles {
        id,
        titles: vec![MovieTitle { iso_3166_1: "DE".into(), title: format!("Film {id}"), r#type: String::new() }],
    }));
    tmdb.expect_image().returning(move |_| Ok(image.clone()));
    tmdb
}

fn action_movie(id: i32) -> MovieDetails {
    MovieDetails { genres: vec![Genre { id: 28, name: "Action".into() }], ..movie_details(id) }
}

/// Image sizes per image, ordered by image id.
async fn sizes_per_image(db: &DbConn) -> Vec<(images::Model, Vec<image_sizes::Model>)> {
    Images::find().order_by_asc(images::Column::Id).find_with_related(ImageSizes).all(db).await.expect("querying images")
}

#[tokio::test]
#[ignore = "requires docker"]
async fn movie_create_persists_movie_with_images_and_sizes() {
    let shared = Arc::new(database().await);
    let db: &DbConn = &shared;
    let f = fakes(&shared);
    seed::language(db, "en", "English").await;
    let action = seed::tmdb_genre(db, 28, "Action", MediaType::Movie).await;
    let user = seed::user(db, 1).await;
    let tmdb = movie_tmdb(action_movie, png_bytes(1000, 1500));

    let (created, id) = movies::create(603, &user, db, &tmdb, &f.store, &f.jobs, &f.clock, &f.random).await
        .ok().expect("create should succeed");

    assert!(created);
    let media = Media::find_by_id(id).one(db).await.expect("querying media").expect("media row");
    assert_eq!((media.tmdb_id, media.r#type.clone(), media.user_id, media.date_added), (Some(603), MediaType::Movie, user.id, date(2024, 1, 1)));
    assert_eq!(media.original_language.as_deref(), Some("en"));
    let movie = Movies::find_by_id(id).one(db).await.expect("querying movies").expect("movie row");
    assert_eq!((movie.release_date, movie.runtime), (Some(date(2024, 1, 1)), Some(100)));

    let titles = media.find_related(Titles).order_by_asc(titles::Column::Id).all(db).await.expect("querying titles");
    let titles: Vec<(String, bool)> = titles.into_iter().map(|t| (t.title, t.primary)).collect();
    assert_eq!(titles, vec![("Movie 603".to_string(), true), ("Film 603".to_string(), false)]);
    let genre_ids: Vec<i32> = media.find_related(Genres).all(db).await.expect("querying genres").iter().map(|g| g.id).collect();
    assert_eq!(genre_ids, vec![action.id]);

    let images = sizes_per_image(db).await;
    let types: Vec<ImageType> = images.iter().map(|(image, _)| image.r#type.clone()).collect();
    assert_eq!(types, vec![ImageType::Backdrop, ImageType::Poster]);
    assert_eq!((media.backdrop_image_id, media.poster_image_id), (Some(images[0].0.id), Some(images[1].0.id)));
    for (image, sizes) in &images {
        assert_eq!(sizes.len(), 5, "sizes of image {}", image.id);
        assert!(sizes.iter().any(|s| (s.width, s.height) == (1000, 1500)), "image {} keeps its original size", image.id);
        let files = f.store.files().into_iter().filter(|(directory, _)| *directory == image.path).count();
        assert_eq!(files, 5, "files of image {}", image.id);
    }
}

#[tokio::test]
#[ignore = "requires docker"]
async fn series_create_persists_seasons_and_episodes() {
    let shared = Arc::new(database().await);
    let db: &DbConn = &shared;
    let f = fakes(&shared);
    seed::language(db, "en", "English").await;
    let user = seed::user(db, 1).await;
    let mut tmdb = MockTmdb::new();
    tmdb.expect_series_details().returning(|id| Ok(SeriesDetails {
        seasons: Some(vec![Season { season_number: Some(1), ..Season::default() }]),
        ..series_details(id)
    }));
    tmdb.expect_season_details().returning(|series_id, number| Ok(SeasonDetails {
        episodes: vec![tmdb_episode(number, 1), tmdb_episode(number, 2)],
        ..season_details(series_id, number)
    }));
    tmdb.expect_series_alternative_titles().returning(|id| Ok(SeriesTitles { id, results: Vec::new() }));
    tmdb.expect_image().returning(|_| Ok(png_bytes(300, 450)));

    let (created, id) = series::create(1399, &user, db, &tmdb, &f.store, &f.jobs, &f.clock, &f.random).await
        .ok().expect("create should succeed");

    assert!(created);
    assert!(Series::find_by_id(id).one(db).await.expect("querying series").is_some());
    let seasons = Seasons::find().filter(seasons::Column::SeriesId.eq(id)).all(db).await.expect("querying seasons");
    assert_eq!(seasons.len(), 1);
    let season = &seasons[0];
    assert_eq!((season.season_number, season.tmdb_id), (Some(1), Some(1399 * 100 + 1)));
    assert!(season.poster_image_id.is_some());
    let episodes = Episodes::find().filter(episodes::Column::SeasonId.eq(season.id))
        .order_by_asc(episodes::Column::EpisodeNumber).all(db).await.expect("querying episodes");
    let numbers: Vec<(Option<i32>, Option<i32>)> = episodes.iter().map(|e| (e.episode_number, e.tmdb_id)).collect();
    assert_eq!(numbers, vec![(Some(1), Some(1001)), (Some(2), Some(1002))]);
    assert!(episodes.iter().all(|e| e.still_image_id.is_some()));

    let images = sizes_per_image(db).await;
    let types: Vec<ImageType> = images.iter().map(|(image, _)| image.r#type.clone()).collect();
    assert_eq!(types, vec![ImageType::Backdrop, ImageType::Poster, ImageType::Poster, ImageType::Still, ImageType::Still]);
    assert!(images.iter().all(|(_, sizes)| sizes.len() == 2), "a 300x450 image has two sizes");
}

/// Movie 1 and movie 2 share a poster; movie 1 also has its own backdrop, a tag and a
/// user-defined genre that no other media has.
#[tokio::test]
#[ignore = "requires docker"]
async fn media_delete_removes_rows_and_unused_images() {
    let shared = Arc::new(database().await);
    let db: &DbConn = &shared;
    let f = fakes(&shared);
    seed::language(db, "en", "English").await;
    seed::tmdb_genre(db, 28, "Action", MediaType::Movie).await;
    let user = seed::user(db, 1).await;
    let tmdb = movie_tmdb(|id| MovieDetails {
        poster_path: Some("/shared-poster.jpg".into()),
        backdrop_path: (id == 1).then(|| "/movie-1-backdrop.jpg".to_string()),
        ..action_movie(id)
    }, png_bytes(300, 450));
    let (_, deleted_id) = movies::create(1, &user, db, &tmdb, &f.store, &f.jobs, &f.clock, &f.random).await.ok().expect("create movie 1");
    let (_, kept_id) = movies::create(2, &user, db, &tmdb, &f.store, &f.jobs, &f.clock, &f.random).await.ok().expect("create movie 2");
    seed::tag(db, deleted_id, "Favourite").await;
    seed::genre(db, deleted_id, "Heist", MediaType::Movie, None).await;
    let deleted = Media::find_by_id(deleted_id).one(db).await.expect("querying media").expect("movie 1");
    let backdrop = Images::find_by_id(deleted.backdrop_image_id.expect("backdrop")).one(db).await.expect("querying images").expect("backdrop row");
    let poster_id = deleted.poster_image_id.expect("poster");

    media::delete(deleted_id, ViewMediaType::Movie, &user, db, &f.store).await.ok().expect("delete should succeed");

    assert!(Media::find_by_id(deleted_id).one(db).await.expect("querying media").is_none());
    assert!(Movies::find_by_id(deleted_id).one(db).await.expect("querying movies").is_none());
    assert_eq!(Titles::find().filter(titles::Column::MediaId.eq(deleted_id)).all(db).await.expect("querying titles").len(), 0);
    assert_eq!(seed::count(db, Tags).await, 0, "the orphaned tag is deleted");
    let genres: Vec<String> = Genres::find().order_by_asc(genres::Column::Id).all(db).await.expect("querying genres")
        .into_iter().map(|g| g.name).collect();
    assert_eq!(genres, vec!["Action".to_string()], "the orphaned user genre is deleted, the TMDB genre kept");
    assert_eq!(seed::count(db, MediaGenres).await, 1, "movie 2 keeps its genre");

    let images: Vec<i32> = Images::find().all(db).await.expect("querying images").iter().map(|i| i.id).collect();
    assert_eq!(images, vec![poster_id], "the backdrop is deleted, the poster movie 2 uses is kept");
    assert_eq!(ImageSizes::find().filter(image_sizes::Column::ImageId.eq(backdrop.id)).all(db).await.expect("querying sizes").len(), 0);
    assert!(f.store.files().iter().all(|(directory, _)| *directory != backdrop.path), "the backdrop's files are deleted");
    assert_eq!(f.store.files().len(), 2, "the poster's two files are kept");
    assert!(Media::find_by_id(kept_id).one(db).await.expect("querying media").is_some());
}

#[tokio::test]
#[ignore = "known bug: media::delete_w_source deletes the media without deleting its unused images when the user is not a bot"]
async fn media_delete_w_source_removes_unused_images_when_user_is_not_a_bot() {
    let shared = Arc::new(database().await);
    let db: &DbConn = &shared;
    let f = fakes(&shared);
    seed::language(db, "en", "English").await;
    let user = seed::user(db, 1).await;
    let tmdb = movie_tmdb(movie_details, png_bytes(300, 450));
    let (_, id) = movies::create(603, &user, db, &tmdb, &f.store, &f.jobs, &f.clock, &f.random).await.ok().expect("create movie");
    seed::source(db, id, "Jellyfin").await;
    assert_eq!(seed::count(db, Images).await, 2);

    media::delete_w_source(603, "Jellyfin".into(), ViewMediaType::Movie, &user, db).await.ok().expect("delete should succeed");

    assert!(Media::find_by_id(id).one(db).await.expect("querying media").is_none());
    assert_eq!(seed::count(db, Images).await, 0, "the movie's backdrop and poster are deleted");
    assert_eq!(seed::count(db, ImageSizes).await, 0);
    assert!(f.store.files().is_empty(), "the images' files are deleted");
}
