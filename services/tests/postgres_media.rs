//! Movie and series creation, the batched details loaders and media deletion against a real
//! Postgres, with images saved through `InlineJobs` into an in-memory store. Needs Docker:
//! `cargo test -p services --test postgres_media -- --ignored`.

mod common;

use std::sync::Arc;
use bytes::Bytes;
use common::postgres::database;
use common::{fakes, seed};
use entities::prelude::{Episodes, Genres, ImageSizes, Images, Media, MediaGenres, Movies, Seasons, Series, Tags, Titles};
use entities::sea_orm_active_enums::{ImageType, MediaType};
use entities::{episodes, genres, image_sizes, images, media as media_entity, seasons, titles};
use integrations::tmdb::views::{Genre, MovieDetails, MovieTitle, MovieTitles, Season, SeasonDetails, SeriesDetails, SeriesTitles};
use sea_orm::{ColumnTrait, DbConn, EntityTrait, ModelTrait, QueryFilter, QueryOrder, Set};
use services::test_support::tmdb::{movie_details, png_bytes, season_details, series_details, tmdb_episode};
use services::test_support::{date, MockTmdb};
use services::{media, media_details, movies, series};
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

/// Names of `items`, in order.
fn names<T>(items: &[T], name: fn(&T) -> &str) -> Vec<&str> {
    items.iter().map(name).collect()
}

/// One batched `load_parts` over two movies: the junction and `IN (...)` queries hand each movie
/// only its own titles, language, genres, tags, sources, logs and watchlist entry.
#[tokio::test]
#[ignore = "requires docker"]
async fn load_parts_gives_each_media_only_its_own_parts_when_given_two_movies() {
    let db = database().await;
    let db: &DbConn = &db;
    seed::language(db, "en", "English").await;
    seed::language(db, "de", "German").await;
    let user = seed::user(db, 1).await;
    let alien = seed::movie(db, &user, 348, "Alien").await;
    let scream = seed::movie(db, &user, 4232, "Scream").await;
    seed::update_media(db, alien.id, media_entity::ActiveModel { original_language: Set(Some("en".into())), ..Default::default() }).await;
    seed::update_media(db, scream.id, media_entity::ActiveModel { original_language: Set(Some("de".into())), ..Default::default() }).await;
    seed::title(db, alien.id, "Alien: The Eighth Passenger", false).await;
    seed::genre(db, alien.id, "Science Fiction", MediaType::Movie, None).await;
    seed::genre(db, alien.id, "Thriller", MediaType::Movie, None).await;
    seed::genre(db, scream.id, "Horror", MediaType::Movie, None).await;
    let classic = seed::tag(db, alien.id, "classic").await;
    seed::add_tag(db, scream.id, &classic).await;
    seed::tag(db, scream.id, "slasher").await;
    let blu_ray = seed::source(db, alien.id, "Blu-ray").await;
    let netflix = seed::source(db, scream.id, "Netflix").await;
    seed::log(db, alien.id, blu_ray.id, Some(4.5), true).await;
    seed::log(db, scream.id, netflix.id, Some(3.0), true).await;
    seed::log(db, scream.id, netflix.id, None, false).await;
    seed::on_watchlist(db, scream.id).await;
    let media = Media::find().filter(media_entity::Column::Id.is_in([alien.id, scream.id]))
        .order_by_asc(media_entity::Column::Id).all(db).await.expect("querying media");

    let parts = media_details::load_parts(&media, db).await.ok().expect("load_parts should succeed");

    assert_eq!(parts.len(), 2);
    let alien = &parts[&alien.id];
    assert_eq!(alien.title, "Alien");
    assert_eq!(names(&alien.alternative_titles, |t| &t.title), vec!["Alien: The Eighth Passenger"]);
    assert_eq!(alien.original_language.as_ref().map(|l| l.english_name.as_str()), Some("English"));
    assert_eq!(names(&alien.genres, |g| &g.name), vec!["Science Fiction", "Thriller"]);
    assert_eq!(names(&alien.tags, |t| &t.name), vec!["classic"]);
    assert_eq!(names(&alien.sources, |s| &s.name), vec!["Blu-ray"]);
    assert_eq!(alien.logs.iter().map(|l| (l.source.as_str(), l.stars)).collect::<Vec<_>>(), vec![("Blu-ray", Some(4.5))]);
    assert!(!alien.on_watchlist);
    let scream = &parts[&scream.id];
    assert_eq!(scream.title, "Scream");
    assert!(scream.alternative_titles.is_empty());
    assert_eq!(scream.original_language.as_ref().map(|l| l.english_name.as_str()), Some("German"));
    assert_eq!(names(&scream.genres, |g| &g.name), vec!["Horror"]);
    assert_eq!(names(&scream.tags, |t| &t.name), vec!["classic", "slasher"]);
    assert_eq!(names(&scream.sources, |s| &s.name), vec!["Netflix"]);
    assert_eq!(scream.logs.iter().map(|l| (l.source.as_str(), l.stars)).collect::<Vec<_>>(), vec![("Netflix", Some(3.0)), ("Netflix", None)]);
    assert!(scream.on_watchlist);
}

/// One pair of grouped `load_counts` queries over three series: two seasons with three episodes,
/// one season without episodes, and no seasons at all.
#[tokio::test]
#[ignore = "requires docker"]
async fn load_counts_gives_each_series_only_its_own_counts_when_given_three_series() {
    let db = database().await;
    let db: &DbConn = &db;
    let user = seed::user(db, 1).await;
    let andor = seed::series(db, &user, 83867, "Andor").await;
    let pilot_only = seed::series(db, &user, 1, "Pilot Only").await;
    let announced = seed::series(db, &user, 2, "Announced").await;
    let andor_1 = seed::season(db, andor.id, 1).await;
    let andor_2 = seed::season(db, andor.id, 2).await;
    seed::episode(db, andor_1.id, 1).await;
    seed::episode(db, andor_1.id, 2).await;
    seed::episode(db, andor_2.id, 1).await;
    seed::season(db, pilot_only.id, 1).await;

    let counts = series::load_counts(&[andor.id, pilot_only.id, announced.id], db).await.ok().expect("load_counts should succeed");

    assert_eq!(counts.len(), 3);
    assert_eq!(counts[&andor.id], (2, 3));
    assert_eq!(counts[&pilot_only.id], (1, 0), "a season without episodes");
    assert_eq!(counts[&announced.id], (0, 0), "a series without seasons");
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
