use entities::prelude::{ImageSizes, Languages, Media, MediaLocks, MovieLocks, Movies, Titles};
use entities::sea_orm_active_enums::{ImageType, MediaType};
use entities::traits::linked::MediaPosters;
use entities::{functions, genres, image_sizes, media, media_genres, movie_locks, movies, titles};
use chrono::NaiveDate;
use integrations::tmdb::views::PropertyChanges;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel as SeaOrmIntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait};
use views::media::{MediaIndex, PageReq};
use views::movies::{MovieDetails, MovieMetadata, MoviePreview};
use views::users::CurrentUser;

use crate::infrastructure::traits::IntoActiveModel;
use crate::infrastructure::{Clock, ImageStore, Random, RuleViolation, SrvErr, Tmdb};
use crate::jobs::Jobs;
use crate::media::{fetch_media, is_locked, try_set_media_lock};
use crate::services;
use entities::traits::locks::{SetLock, ToLocks};
use crate::services::images::{fetch_backdrop_and_poster, save_tmdb_image};
use crate::services::media_details::{load_parts, MediaParts};
use crate::services::media_preview::{library_id, load_genres, load_language, non_empty, parse_date, tmdb_error, tmdb_images, Preview};
use std::collections::HashMap;
use views::images::Image;

#[allow(clippy::too_many_arguments)]
pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, clock: &dyn Clock, random: &dyn Random) -> Result<(bool, i32), SrvErr> {
    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie)).one(db).await;
    match med_res {
        Ok(media) => {
            if let Some(media) = media {
                return Ok((false, media.id));
            }
        }
        Err(err) => { return Err(SrvErr::DB(err)); }
    };


    let tmdb_movie = match tmdb.movie_details(tmdb_id).await {
        Ok(movie) => { movie }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let titles = tmdb.movie_alternative_titles(tmdb_id).await?;

    let backdrop_image_id = match &tmdb_movie.backdrop_path {
        None => None,
        Some(path) => Some(save_tmdb_image(path.as_str(), ImageType::Backdrop, db, tmdb, store, jobs, random).await?),
    };
    let poster_image_id = match &tmdb_movie.poster_path {
        None => None,
        Some(path) => Some(save_tmdb_image(path.as_str(), ImageType::Poster, db, tmdb, store, jobs, random).await?),
    };

    let tran = db.begin().await?;

    let mut media: media::ActiveModel = tmdb_movie.into_active_model();
    let mut movie: movies::ActiveModel = tmdb_movie.into_active_model();

    media.backdrop_image_id = Set(backdrop_image_id);
    media.poster_image_id = Set(poster_image_id);
    media.date_added = Set(clock.now().date_naive());
    media.user_id = Set(user.id);
    media.bot_controllable = Set(user.though_bot);
    let inserted_media = media.insert(&tran).await?;
    movie.id = Set(inserted_media.id);
    movie.insert(&tran).await?;

    for genre in &tmdb_movie.genres {
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id))
            .filter(genres::Column::Type.eq(MediaType::Movie)).one(&tran).await?;
        if existing.is_none() {
            return Err(SrvErr::MasterdataOutdated);
        }

        let media_genre = media_genres::ActiveModel {
            media_id: Set(inserted_media.id),
            genre_id: Set(existing.unwrap().id),
        };
        media_genre.insert(&tran).await?;
    }


    let model = titles::ActiveModel {
        id: NotSet,
        media_id: Set(inserted_media.id),
        primary: Set(true),
        title: Set(tmdb_movie.title)
    };
    model.insert(&tran).await?;

    for title in titles.titles {
        let model = titles::ActiveModel {
            id: NotSet,
            media_id: Set(inserted_media.id),
            primary: Set(false),
            title: Set(title.title)
        };
        model.insert(&tran).await?;
    }

    tran.commit().await?;
    Ok((true, inserted_media.id))
}


/// What TMDB says about movie `tmdb_id`, for a movie that is not in the user's library; or the id of
/// the user's movie when it is, in which case TMDB is not called. Writes nothing. Fails with
/// `MasterdataOutdated` when a genre of the movie is not in the masterdata, as `create` does, and
/// with `NotFound` when TMDB does not know the movie. At most 3 queries.
pub async fn preview(tmdb_id: i32, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb) -> Result<Preview<MoviePreview>, SrvErr> {
    if let Some(id) = library_id(tmdb_id, MediaType::Movie, user, db).await? {
        return Ok(Preview::InLibrary(id));
    }

    let tmdb_movie = tmdb.movie_details(tmdb_id).await.map_err(tmdb_error)?;
    let titles = tmdb.movie_alternative_titles(tmdb_id).await.map_err(tmdb_error)?;

    let genres = load_genres(&tmdb_movie.genres, MediaType::Movie, db).await?;
    let original_language = load_language(tmdb_movie.original_language.as_deref(), db).await?;
    let (poster, backdrop) = tmdb_images(&tmdb_movie.poster_path, &tmdb_movie.backdrop_path);

    Ok(Preview::New(MoviePreview {
        poster,
        backdrop,
        title: tmdb_movie.title,
        alternative_titles: titles.titles.into_iter().map(|t| t.title).collect(),
        release_date: parse_date(tmdb_movie.release_date.as_deref()),
        runtime: tmdb_movie.runtime,
        status: tmdb_movie.status,
        overview: non_empty(tmdb_movie.overview),
        tmdb_id: tmdb_movie.id,
        tmdb_vote_average: tmdb_movie.vote_average,
        original_language,
        genres,
    }))
}

pub async fn index(page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media = Media::find().filter(media::Column::UserId.eq(user.id))
        .find_also_linked(MediaPosters)
        .find_also_related(Titles)
        .filter(media::Column::Type.eq(MediaType::Movie))
        .filter(titles::Column::Primary.eq(true)).order_by_asc(functions::default_media_sort())
        .offset(page_req.offset).limit(page_req.limit).all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let indexes = services::media::build_media_indexes(media, poster_sizes);
    Ok(indexes)
}

pub async fn details(movie_id: i32, user: &CurrentUser, db: &DbConn) -> Result<Option<MovieDetails>, SrvErr> {
    let db_media = Media::find().filter(media::Column::Id.eq(movie_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Movie))
        .one(db).await?;

    if db_media.is_none() {
        return Ok(None);
    }
    let db_media = db_media.unwrap();

    let db_movie = load_movies(&[db_media.id], db).await?.remove(&db_media.id).expect("DB in invalid state!");
    let parts = load_parts(std::slice::from_ref(&db_media), db).await?.remove(&db_media.id).expect("parts of every media");

    let media_locks = db_media.find_related(MediaLocks).one(db).await?;
    let movie_locks = db_movie.find_related(MovieLocks).one(db).await?;
    let mut locks = media_locks.to_locks();
    locks.append(&mut movie_locks.to_locks());

    let (backdrop, poster) = fetch_backdrop_and_poster(&db_media, db).await?;

    Ok(Some(details_view(db_media, db_movie, parts, locks, backdrop, poster)))
}

/// The movie rows of the media `media_ids`, keyed by id, in one query. The caller passes ids of
/// media already filtered by user and type; no ownership is checked here.
pub(crate) async fn load_movies(media_ids: &[i32], db: &DbConn) -> Result<HashMap<i32, movies::Model>, SrvErr> {
    let movies = Movies::find().filter(movies::Column::Id.is_in(media_ids.to_vec())).all(db).await?;
    Ok(movies.into_iter().map(|m| (m.id, m)).collect())
}

/// The details of a movie from its rows and its loaded parts.
pub(crate) fn details_view(media: media::Model, movie: movies::Model, parts: MediaParts, locks: Vec<&'static str>, backdrop: Option<Image>, poster: Option<Image>) -> MovieDetails {
    MovieDetails {
        id: media.id,
        poster,
        backdrop,
        stars: media.stars,
        title: parts.title,
        alternative_titles: parts.alternative_titles,
        release_date: movie.release_date,
        runtime: movie.runtime,
        status: movie.status,
        overview: media.overview,
        tmdb_id: media.tmdb_id,
        tmdb_vote_average: media.tmdb_vote_average,
        on_watchlist: parts.on_watchlist,
        original_language: parts.original_language,
        genres: parts.genres,
        tags: parts.tags,
        sources: parts.sources,
        logs: parts.logs,
        locks
    }
}

pub async fn metadata(movie_id: i32, user: &CurrentUser, db: &DbConn) -> Result<MovieMetadata, SrvErr> {
    let db_media = Media::find().filter(media::Column::Id.eq(movie_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Movie)).one(db).await?;

    if db_media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let db_media = db_media.unwrap();

    let movie = db_media.find_related(Movies).one(db).await?.expect("DB in invalid state!");

    let title = db_media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?.map(|m| { m.title });

    let metadata = MovieMetadata {
        id: movie.id,
        homepage: db_media.homepage,
        imdb_id: db_media.imdb_id,
        title,
        overview: db_media.overview,
        original_language: db_media.original_language,
        origin_country: db_media.origin_country,
        release_date: movie.release_date,
        runtime: movie.runtime,
        status: movie.status,
    };
    Ok(metadata)
}

pub async fn update(movie_id: i32, metadata: MovieMetadata, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if movie_id != metadata.id {
        return Err(SrvErr::BadRequest("Endpoint movie id does not match movie id in metadata!".to_string()));
    }

    let media = Media::find().filter(media::Column::Id.eq(movie_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Movie)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let mut rule_violations = Vec::new();

    if metadata.original_language != media.original_language
        && let Some(language) = metadata.original_language.as_ref() {
        let found = Languages::find_by_id(language).count(db).await?;
        if found == 0 {
            rule_violations.push(RuleViolation::MediaInvalidOriginalLanguage);
        }
    }

    if let Some(runtime) = metadata.runtime
        && runtime < 0 {
            rule_violations.push(RuleViolation::MovieNegativeRuntime);
        }

    if !rule_violations.is_empty() {
        return Err(SrvErr::RuleViolation(rule_violations));
    }

    let tran = db.begin().await?;

    let mut media_am: media::ActiveModel = (&metadata).into_active_model();
    media_am.bot_controllable = sea_orm::Set(media.bot_controllable && user.though_bot);
    media_am.update(&tran).await?;

    let movie_am: movies::ActiveModel = (&metadata).into_active_model();
    movie_am.update(&tran).await?;

    let db_title = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(&tran).await?;

    if let Some(title) = metadata.title {
        if let Some(title_mod) = db_title {
            let mut am = title_mod.into_active_model();
            am.title = Set(title);
            am.update(&tran).await?;
        } else {
            let am = titles::ActiveModel {
                id: NotSet,
                media_id: Set(media.id),
                primary: Set(true),
                title: Set(title),
            };
            am.insert(&tran).await?;
        }
    } else {
        if let Some(title_mod) = db_title {
            title_mod.delete(&tran).await?;
        }
    }

    tran.commit().await?;
    Ok(())
}

pub async fn lock(movie_id: i32, property: String, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    set_lock(movie_id, property, true, user, db).await
}

pub async fn unlock(movie_id: i32, property: String, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    set_lock(movie_id, property, false, user, db).await
}

async fn set_lock(movie_id: i32, property: String, locked: bool, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = fetch_media(movie_id, views::media::MediaType::Movie, user, db).await?;
    if try_set_media_lock(media, &property, locked, db).await? {
        return Ok(());
    }

    if !movie_locks::ActiveModel::has_lock(&property) {
        return Err(SrvErr::BadRequest("Lock or property does not exist!".to_string()));
    }

    let locks = MovieLocks::find().filter(movie_locks::Column::MovieId.eq(movie_id)).one(db).await?;
    if let Some(locks) = locks {
        let mut active_locks = locks.into_active_model();
        active_locks.set_lock(&property, locked);
        active_locks.update(db).await?;
    }
    else {
        let mut active_locks = movie_locks::ActiveModel {
            movie_id: Set(movie_id),
            release_date: Set(false),
            runtime: Set(false),
            status: Set(false),
        };
        active_locks.set_lock(&property, locked);
        active_locks.insert(db).await?;
    }
    Ok(())
}

/// Apply TMDB property-level changes to a movie, respecting locks.
pub async fn apply_changes(media: &entities::media::Model, changes: &PropertyChanges, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<(), SrvErr> {
    crate::media::apply_changes(media, changes, db, tmdb, store, jobs, random).await?;

    let movie_locks = MovieLocks::find()
        .filter(movie_locks::Column::MovieId.eq(media.id))
        .one(db)
        .await?;

    let mut movie_am = movies::ActiveModel {
        id: Set(media.id),
        ..Default::default()
    };
    let mut any_change = false;

    for change in &changes.changes {
        let last_item = match change.items.last() {
            Some(item) => item,
            None => continue,
        };
        let value = match &last_item.value {
            Some(v) => v,
            None => continue,
        };

        match change.key.as_str() {
            "status" => {
                if !is_locked(&movie_locks, |l| l.status) {
                    movie_am.status = Set(value.as_str().map(|s| s.to_string()));
                    any_change = true;
                }
            }
            "runtime" => {
                if !is_locked(&movie_locks, |l| l.runtime) {
                    movie_am.runtime = Set(value.as_i64().map(|n| n as i32));
                    any_change = true;
                }
            }
            "release_dates" => {
                if !is_locked(&movie_locks, |l| l.release_date) {
                    let country = value.get("iso_3166_1").and_then(|v| v.as_str()).unwrap_or("");
                    if media.origin_country.as_ref().is_some_and(|c| c.iter().any(|c| c == country))
                        && let Some(date_str) = value.get("release_date").and_then(|v| v.as_str()) {
                            let date = chrono::DateTime::parse_from_rfc3339(date_str)
                                .map(|dt| dt.date_naive())
                                .or_else(|_| NaiveDate::parse_from_str(date_str, "%Y-%m-%d"))
                                .ok();
                            if let Some(date) = date {
                                movie_am.release_date = Set(Some(date));
                                any_change = true;
                            }
                        }
                }
            }
            _ => {}
        }
    }

    if any_change {
        movie_am.update(db).await?;
    }

    Ok(())
}


#[cfg(test)]
mod tests {
    use entities::images;
    use integrations::tmdb::views::{Genre as TmdbGenre, MovieDetails as TmdbMovieDetails, MovieTitle, MovieTitles};
    use sea_orm::Value;
    use uuid::Uuid;
    use super::*;
    use crate::services::media_preview::Preview;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted, with_saved_tmdb_image};
    use crate::test_support::fixtures;
    use crate::test_support::jobs::sizes;
    use crate::test_support::tmdb::png_bytes;
    use crate::test_support::{date, fixed_clock, InMemoryImageStore, MockTmdb, RecordingJobs, SeqRandom};

    fn movie_genre(id: i32, tmdb_id: i32, name: &str) -> genres::Model {
        genres::Model { tmdb_id: Some(tmdb_id), ..fixtures::genre_model(id, name, MediaType::Movie) }
    }

    /// Runs `create` with a fixed clock and sequential randomness.
    async fn run_create(tmdb_id: i32, user: &CurrentUser, db: &DbConn, tmdb: &MockTmdb, store: &InMemoryImageStore, jobs: &RecordingJobs) -> Result<(bool, i32), SrvErr> {
        create(tmdb_id, user, db, tmdb, store, jobs, &fixed_clock(2026, 3, 4, 12, 0, 0), &SeqRandom::new()).await
    }

    /// A TMDB answering `movie_details` with a movie without images or genres, and no alternative titles.
    fn bare_movie_tmdb(tmdb_id: i32) -> MockTmdb {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1).returning(|id| Ok(TmdbMovieDetails {
            backdrop_path: None, poster_path: None, ..test_support::tmdb::movie_details(id)
        }));
        tmdb.expect_movie_alternative_titles().times(1)
            .returning(move |_| Ok(MovieTitles { id: tmdb_id, titles: Vec::new() }));
        tmdb
    }

    #[tokio::test]
    async fn create_returns_existing_id_when_movie_already_added() {
        test_support::init();
        let tmdb = MockTmdb::new();
        let db = test_support::db::mock()
            .append_query_results([[media::Model { tmdb_id: Some(603), ..fixtures::movie_media_model(5, 1) }]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(603, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        assert!(matches!(result, Ok((false, 5))));
        assert!(jobs.take().is_empty());
        assert!(store.files().is_empty());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(603), Value::from(1)]);
    }

    #[tokio::test]
    async fn create_inserts_media_movie_titles_and_genres() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().withf(|id| *id == 603).times(1).returning(|id| Ok(TmdbMovieDetails {
            genres: vec![TmdbGenre { id: 28, name: "Action".into() }, TmdbGenre { id: 878, name: "Science Fiction".into() }],
            ..test_support::tmdb::movie_details(id)
        }));
        tmdb.expect_movie_alternative_titles().withf(|id| *id == 603).times(1).returning(|id| Ok(MovieTitles {
            id,
            titles: vec![MovieTitle { iso_3166_1: "DE".into(), title: "Matrix Deutsch".into(), r#type: String::new() }],
        }));
        tmdb.expect_image().withf(|path| path == "/movie-603-backdrop.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        tmdb.expect_image().withf(|path| path == "/movie-603-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()]);
        // The backdrop, then the poster image
        let db = with_saved_tmdb_image(db, 11, ImageType::Backdrop);
        let db = with_saved_tmdb_image(db, 12, ImageType::Poster)
            // media, movie, then per genre: lookup and link; then titles
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::movie_model(10)]])
            .append_query_results([[movie_genre(5, 28, "Action")]])
            .append_query_results([[fixtures::media_genre_model(10, 5)]])
            .append_query_results([[movie_genre(6, 878, "Science Fiction")]])
            .append_query_results([[fixtures::media_genre_model(10, 6)]])
            .append_query_results([[fixtures::title_model(1, 10, "Movie 603", true)]])
            .append_query_results([[fixtures::title_model(2, 10, "Matrix Deutsch", false)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(603, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        // The clock's date, the user, and not bot controllable; the saved images' ids.
        assert_wrote(&log, r#"INSERT INTO "media""#, &[
            Value::from(date(2026, 3, 4)), Value::from(1), Value::from(false),
            Value::from(603), Value::from(11), Value::from(12),
        ]);
        assert_wrote(&log, r#"INSERT INTO "movies""#, &[
            Value::from(10), Value::from(date(2024, 1, 1)), Value::from(100), Value::from("Released"),
        ]);
        assert_wrote(&log, r#"INSERT INTO "media_genres""#, &[Value::from(10), Value::from(5)]);
        assert_wrote(&log, r#"INSERT INTO "media_genres""#, &[Value::from(10), Value::from(6)]);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(10), Value::from("Movie 603"), Value::from(true)]);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(10), Value::from("Matrix Deutsch"), Value::from(false)]);
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/movie-603-backdrop.jpg")]);
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/movie-603-poster.jpg")]);
        assert_wrote(&log, "COMMIT", &[]);
        assert_eq!(store.files(), vec![
            (Uuid::from_u128(1000).simple().to_string(), "300x450.png".to_string()),
            (Uuid::from_u128(1001).simple().to_string(), "300x450.png".to_string()),
        ]);
        // One extra size per image; the jobs are not run here (covered by the job handler tests).
        assert_eq!(sizes(&jobs.take()), vec![(11, 100, 150), (12, 100, 150)]);
    }

    #[tokio::test]
    async fn create_marks_media_bot_controllable_when_created_through_bot() {
        test_support::init();
        let tmdb = bare_movie_tmdb(603);
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::movie_model(10)]])
            .append_query_results([[fixtures::title_model(1, 10, "Movie 603", true)]])
            .into_connection();

        let result = run_create(603, &fixtures::bot_user(), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "media""#, &[Value::from(true), Value::from(1)]);
    }

    #[tokio::test]
    async fn create_returns_masterdata_outdated_when_genre_unknown() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1).returning(|id| Ok(TmdbMovieDetails {
            backdrop_path: None,
            poster_path: None,
            genres: vec![TmdbGenre { id: 28, name: "Action".into() }],
            ..test_support::tmdb::movie_details(id)
        }));
        tmdb.expect_movie_alternative_titles().times(1).returning(|id| Ok(MovieTitles { id, titles: Vec::new() }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::movie_model(10)]])
            .append_query_results([Vec::<genres::Model>::new()])
            .into_connection();

        let result = run_create(603, &fixtures::user(), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(matches!(result, Err(SrvErr::MasterdataOutdated)));
        let log = test_support::db::log(db);
        assert_wrote(&log, "ROLLBACK", &[]);
        assert_no_statement_starting_with(&log, "COMMIT");
        assert_no_statement_starting_with(&log, r#"INSERT INTO "titles""#);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "media_genres""#);
    }

    #[tokio::test]
    async fn create_returns_err_and_writes_nothing_when_tmdb_details_fail() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1).returning(|_| Err(test_support::tmdb::error("TMDB unavailable")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(603, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
        assert!(store.files().is_empty());
        assert!(jobs.take().is_empty());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    // -------------------------------------------------------------------- preview

    /// A TMDB answering `movie_details` and `movie_alternative_titles` once each, for movie 603
    /// with two genres, a poster, a backdrop and one alternative title.
    fn full_movie_tmdb() -> MockTmdb {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().withf(|id| *id == 603).times(1).returning(|id| Ok(TmdbMovieDetails {
            genres: vec![TmdbGenre { id: 28, name: "Action".into() }, TmdbGenre { id: 878, name: "Science Fiction".into() }],
            ..test_support::tmdb::movie_details(id)
        }));
        tmdb.expect_movie_alternative_titles().withf(|id| *id == 603).times(1).returning(|id| Ok(MovieTitles {
            id,
            titles: vec![MovieTitle { iso_3166_1: "DE".into(), title: "Matrix Deutsch".into(), r#type: String::new() }],
        }));
        tmdb
    }

    #[tokio::test]
    async fn preview_returns_library_id_without_calling_tmdb_when_movie_already_added() {
        test_support::init();
        let tmdb = MockTmdb::new();
        let db = test_support::db::mock()
            .append_query_results([[media::Model { tmdb_id: Some(603), ..fixtures::movie_media_model(5, 1) }]])
            .into_connection();

        let result = preview(603, &fixtures::user(), &db, &tmdb).await;

        assert!(matches!(result, Ok(Preview::InLibrary(5))));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(603), Value::from(1)]);
    }

    #[tokio::test]
    async fn preview_returns_tmdb_data_when_movie_not_in_library() {
        test_support::init();
        let tmdb = full_movie_tmdb();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[movie_genre(5, 28, "Action"), movie_genre(6, 878, "Science Fiction")]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .into_connection();

        let result = preview(603, &fixtures::user(), &db, &tmdb).await;

        let Ok(Preview::New(movie)) = result else { panic!("expected a preview") };
        assert_eq!(movie.tmdb_id, 603);
        assert_eq!(movie.title, "Movie 603");
        assert_eq!(movie.alternative_titles.iter().map(String::as_str).collect::<Vec<_>>(), vec!["Matrix Deutsch"]);
        assert_eq!(movie.release_date, Some(date(2024, 1, 1)));
        assert_eq!(movie.runtime, Some(100));
        assert_eq!(movie.status.as_deref(), Some("Released"));
        assert_eq!(movie.overview.as_deref(), Some("Overview of movie 603"));
        assert_eq!(movie.tmdb_vote_average, Some(7.5));
        assert_eq!(movie.original_language.as_ref().map(|l| l.english_name.as_str()), Some("English"));
        assert_eq!(movie.genres.iter().map(|g| (g.id, g.name.as_str())).collect::<Vec<_>>(), vec![(5, "Action"), (6, "Science Fiction")]);
        let poster = movie.poster.expect("poster should be set");
        assert_eq!(poster.path, "/movie-603-poster.jpg");
        assert_eq!(poster.sizes.iter().map(|s| s.slug.as_str()).collect::<Vec<_>>(),
                   vec!["tmdb/w92", "tmdb/w154", "tmdb/w185", "tmdb/w342", "tmdb/w500", "tmdb/w780", "tmdb/original"]);
        let backdrop = movie.backdrop.expect("backdrop should be set");
        assert_eq!(backdrop.path, "/movie-603-backdrop.jpg");
        assert_eq!(backdrop.sizes.iter().map(|s| s.slug.as_str()).collect::<Vec<_>>(),
                   vec!["tmdb/w300", "tmdb/w780", "tmdb/w1280", "tmdb/original"]);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
        assert_no_statement_starting_with(&log, "INSERT");
        assert_no_statement_starting_with(&log, "UPDATE");
        assert_no_statement_starting_with(&log, "DELETE");
    }

    #[tokio::test]
    async fn preview_skips_genre_and_language_queries_when_movie_has_none() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1).returning(|id| Ok(TmdbMovieDetails {
            backdrop_path: None, poster_path: None, original_language: None, overview: Some(String::new()),
            release_date: Some("not a date".into()), ..test_support::tmdb::movie_details(id)
        }));
        tmdb.expect_movie_alternative_titles().times(1).returning(|id| Ok(MovieTitles { id, titles: Vec::new() }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = preview(603, &fixtures::user(), &db, &tmdb).await;

        let Ok(Preview::New(movie)) = result else { panic!("expected a preview") };
        assert!(movie.poster.is_none());
        assert!(movie.backdrop.is_none());
        assert!(movie.original_language.is_none());
        assert!(movie.genres.is_empty());
        assert!(movie.alternative_titles.is_empty());
        assert_eq!(movie.overview, None);
        assert_eq!(movie.release_date, None);
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn preview_returns_masterdata_outdated_when_genre_unknown() {
        test_support::init();
        let tmdb = full_movie_tmdb();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[movie_genre(5, 28, "Action")]])
            .into_connection();

        let result = preview(603, &fixtures::user(), &db, &tmdb).await;

        assert!(matches!(result, Err(SrvErr::MasterdataOutdated)));
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn preview_returns_not_found_when_tmdb_answers_404() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1)
            .returning(|_| Err(test_support::tmdb::error_with_status(http::StatusCode::NOT_FOUND)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = preview(603, &fixtures::user(), &db, &tmdb).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn preview_returns_integration_err_when_tmdb_fails() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().times(1).returning(|_| Err(test_support::tmdb::error("TMDB unavailable")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = preview(603, &fixtures::user(), &db, &tmdb).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
    }

    #[tokio::test]
    async fn index_returns_page_of_movie_indexes() {
        test_support::init();
        let with_poster = media::Model { poster_image_id: Some(2), stars: Some(4.5), ..fixtures::movie_media_model(1, 1) };
        let without_poster = fixtures::movie_media_model(3, 1);
        let db = test_support::db::mock()
            .append_query_results([vec![
                fixtures::media_index_row(with_poster, Some(fixtures::image_model(2)), Some(fixtures::title_model(1, 1, "Arrival", true))),
                fixtures::media_index_row(without_poster, None, Some(fixtures::title_model(3, 3, "Brazil", true))),
            ]])
            .append_query_results([[fixtures::image_size_model(1, 2, 300, 450), fixtures::image_size_model(2, 2, 100, 150)]])
            .into_connection();

        let page = PageReq { offset: Some(20), limit: Some(10) };
        let result = index(page, &fixtures::user(), &db).await.expect("index should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[0].title, "Arrival");
        assert_eq!(result[0].stars, Some(4.5));
        let poster = result[0].poster.as_ref().expect("first movie has a poster");
        assert_eq!(poster.path, "/image-2.jpg");
        assert_eq!(poster.sizes.len(), 2);
        assert_eq!(result[1].id, 3);
        assert_eq!(result[1].title, "Brazil");
        assert!(result[1].poster.is_none());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, "SELECT", &[Value::from(1), Value::from(20u64), Value::from(10u64)]);
    }

    #[tokio::test]
    async fn details_returns_none_when_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = details(7, &fixtures::user(), &db).await.expect("details should succeed");

        assert!(result.is_none());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(7), Value::from(1)]);
    }

    #[tokio::test]
    async fn details_assembles_titles_genres_tags_sources_logs_and_images() {
        test_support::init();
        let media = media::Model {
            tmdb_id: Some(603),
            overview: Some("A hacker learns the truth.".into()),
            tmdb_vote_average: Some(8.2),
            original_language: Some("en".into()),
            stars: Some(4.5),
            poster_image_id: Some(2),
            ..fixtures::movie_media_model(1, 1)
        };
        let log_row = entities::logs::Model { stars: Some(4.5), comment: Some("Great".into()), ..fixtures::log_model(1, 1, 9) };
        let db = test_support::db::mock()
            .append_query_results([[media]])
            .append_query_results([[fixtures::movie_model(1)]])
            .append_query_results([[
                fixtures::title_model(1, 1, "The Matrix", true),
                fixtures::title_model(2, 1, "Matrix", false),
            ]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([[(fixtures::media_genre_model(1, 3), Some(fixtures::genre_model(3, "Action", MediaType::Movie)))]])
            .append_query_results([[(fixtures::media_tag_model(1, 4), Some(fixtures::tag_model(4, "classic")))]])
            .append_query_results([[fixtures::source_model(9, 1, "Blu-ray")]])
            .append_query_results([[log_row]])
            .append_query_results([[fixtures::watchlist_model(1)]])
            .append_query_results([[entities::media_locks::Model { overview: true, ..fixtures::media_locks_model(1) }]])
            .append_query_results([[entities::movie_locks::Model { runtime: true, ..fixtures::movie_locks_model(1) }]])
            .append_query_results([[(fixtures::image_model(2), Some(fixtures::image_size_model(1, 2, 300, 450)))]])
            .into_connection();

        let movie = details(1, &fixtures::user(), &db).await.expect("details should succeed")
            .expect("movie should be found");

        assert_eq!(movie.id, 1);
        assert_eq!(movie.title, "The Matrix");
        assert_eq!(movie.alternative_titles.len(), 1);
        assert_eq!(movie.alternative_titles[0].id, 2);
        assert_eq!(movie.alternative_titles[0].title, "Matrix");
        assert_eq!(movie.tmdb_id, Some(603));
        assert_eq!(movie.overview.as_deref(), Some("A hacker learns the truth."));
        assert_eq!(movie.stars, Some(4.5));
        assert_eq!(movie.release_date, Some(date(2024, 1, 1)));
        assert_eq!(movie.runtime, Some(100));
        assert_eq!(movie.status.as_deref(), Some("Released"));
        assert!(movie.on_watchlist);
        assert_eq!(movie.original_language.as_ref().map(|l| l.english_name.as_str()), Some("English"));
        assert_eq!(movie.genres.len(), 1);
        assert_eq!(movie.genres[0].name, "Action");
        assert_eq!(movie.tags.len(), 1);
        assert_eq!(movie.tags[0].name, "classic");
        assert_eq!(movie.sources.len(), 1);
        assert_eq!(movie.sources[0].name, "Blu-ray");
        assert_eq!(movie.logs.len(), 1);
        assert_eq!(movie.logs[0].source, "Blu-ray");
        assert_eq!(movie.logs[0].stars, Some(4.5));
        assert_eq!(movie.logs[0].comment.as_deref(), Some("Great"));
        assert_eq!(movie.locks, vec!["overview", "runtime"]);
        let poster = movie.poster.expect("movie has a poster");
        assert_eq!(poster.path, "/image-2.jpg");
        assert_eq!(poster.sizes.len(), 1);
        assert!(movie.backdrop.is_none());
    }

    #[tokio::test]
    async fn details_uses_unset_title_when_no_primary_title() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(1, 1)]])
            .append_query_results([[fixtures::movie_model(1)]])
            .append_query_results([[fixtures::title_model(2, 1, "Only Alternative", false)]])
            .append_query_results([Vec::<(media_genres::Model, Option<genres::Model>)>::new()])
            .append_query_results([Vec::<(entities::media_tags::Model, Option<entities::tags::Model>)>::new()])
            .append_query_results([Vec::<entities::sources::Model>::new()])
            .append_query_results([Vec::<entities::logs::Model>::new()])
            .append_query_results([Vec::<entities::watchlist::Model>::new()])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<movie_locks::Model>::new()])
            .append_query_results([Vec::<(images::Model, Option<image_sizes::Model>)>::new()])
            .into_connection();

        let movie = details(1, &fixtures::user(), &db).await.expect("details should succeed")
            .expect("movie should be found");

        assert_eq!(movie.title, "Unknown");
        assert_eq!(movie.alternative_titles.len(), 1);
        assert!(!movie.on_watchlist);
        assert!(movie.original_language.is_none());
        assert!(movie.locks.is_empty());
        assert!(movie.poster.is_none());
        assert!(movie.backdrop.is_none());
    }

    #[tokio::test]
    async fn metadata_returns_current_values_without_reading_locks() {
        test_support::init();
        let media = media::Model {
            homepage: Some("https://matrix.example.test".into()),
            imdb_id: Some("tt0133093".into()),
            overview: Some("A hacker learns the truth.".into()),
            original_language: Some("en".into()),
            origin_country: Some(vec!["US".into()]),
            ..fixtures::movie_media_model(1, 1)
        };
        let db = test_support::db::mock()
            .append_query_results([[media]])
            .append_query_results([[fixtures::movie_model(1)]])
            .append_query_results([[fixtures::title_model(1, 1, "The Matrix", true)]])
            .into_connection();

        let metadata = metadata(1, &fixtures::user(), &db).await.expect("metadata should succeed");

        assert_eq!(metadata.id, 1);
        assert_eq!(metadata.title.as_deref(), Some("The Matrix"));
        assert_eq!(metadata.homepage.as_deref(), Some("https://matrix.example.test"));
        assert_eq!(metadata.imdb_id.as_deref(), Some("tt0133093"));
        assert_eq!(metadata.overview.as_deref(), Some("A hacker learns the truth."));
        assert_eq!(metadata.original_language.as_deref(), Some("en"));
        assert_eq!(metadata.origin_country, Some(vec!["US".to_string()]));
        assert_eq!(metadata.release_date, Some(date(2024, 1, 1)));
        assert_eq!(metadata.runtime, Some(100));
        assert_eq!(metadata.status.as_deref(), Some("Released"));
        // The metadata view carries no lock information (`details` does); no lock rows are read.
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
        assert!(log.iter().all(|s| !s.sql.contains("_locks")), "metadata should not read locks");
    }

    #[tokio::test]
    async fn metadata_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = metadata(1, &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    fn movie_metadata(id: i32) -> MovieMetadata {
        MovieMetadata {
            id,
            homepage: Some("https://new.example.test".into()),
            imdb_id: Some("tt0000001".into()),
            title: Some("New Title".into()),
            overview: Some("New overview".into()),
            original_language: None,
            origin_country: Some(vec!["CA".into()]),
            release_date: Some(date(2025, 6, 7)),
            runtime: Some(133),
            status: Some("post production".into()),
        }
    }

    #[tokio::test]
    async fn update_writes_changed_fields_without_touching_locks() {
        test_support::init();
        let media = media::Model { bot_controllable: true, ..fixtures::movie_media_model(1, 1) };
        let db = test_support::db::mock()
            .append_query_results([[media.clone()]])
            .append_query_results([[media]])
            .append_query_results([[fixtures::movie_model(1)]])
            .append_query_results([[fixtures::title_model(1, 1, "Old Title", true)]])
            .append_query_results([[fixtures::title_model(1, 1, "New Title", true)]])
            .into_connection();

        let result = update(1, movie_metadata(1), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[
            Value::from("https://new.example.test"), Value::from("tt0000001"), Value::from("New overview"),
            // A user who is not a bot takes over the media from the bot.
            Value::from(false),
        ]);
        assert_wrote(&log, r#"UPDATE "movies""#, &[
            Value::from(date(2025, 6, 7)), Value::from(133), Value::from("Post Production"),
        ]);
        assert_wrote(&log, r#"UPDATE "titles""#, &[Value::from("New Title")]);
        // Editing does not touch the lock tables; locks are set through `lock`.
        assert_no_statement_starting_with(&log, "INSERT");
        assert!(log.iter().all(|s| !s.sql.contains("_locks")), "update should not read or write locks");
    }

    #[tokio::test]
    async fn update_keeps_bot_controllable_when_bot_edits_bot_controllable_media() {
        test_support::init();
        let media = media::Model { bot_controllable: true, ..fixtures::movie_media_model(1, 1) };
        let db = test_support::db::mock()
            .append_query_results([[media.clone()]])
            .append_query_results([[media]])
            .append_query_results([[fixtures::movie_model(1)]])
            .append_query_results([[fixtures::title_model(1, 1, "Old Title", true)]])
            .append_query_results([[fixtures::title_model(1, 1, "New Title", true)]])
            .into_connection();

        let result = update(1, movie_metadata(1), &fixtures::bot_user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from("New overview"), Value::from(true)]);
    }

    #[tokio::test]
    async fn update_creates_primary_title_when_media_has_none() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(1, 1)]])
            .append_query_results([[fixtures::movie_media_model(1, 1)]])
            .append_query_results([[fixtures::movie_model(1)]])
            .append_query_results([Vec::<titles::Model>::new()])
            .append_query_results([[fixtures::title_model(5, 1, "New Title", true)]])
            .into_connection();

        let result = update(1, movie_metadata(1), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(1), Value::from("New Title"), Value::from(true)]);
    }

    #[tokio::test]
    async fn update_deletes_primary_title_when_metadata_has_no_title() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(1, 1)]])
            .append_query_results([[fixtures::movie_media_model(1, 1)]])
            .append_query_results([[fixtures::movie_model(1)]])
            .append_query_results([[fixtures::title_model(5, 1, "Old Title", true)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = update(1, MovieMetadata { title: None, ..movie_metadata(1) }, &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "titles""#, &[Value::from(5)]);
    }

    #[tokio::test]
    async fn update_returns_bad_request_when_ids_differ() {
        test_support::init();
        let db = test_support::db::mock().into_connection();

        let result = update(1, movie_metadata(2), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(_))));
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn update_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        // The query filters on the user, so the other user's movie is not returned.
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = update(5, movie_metadata(5), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(5), Value::from(1)]);
    }

    #[tokio::test]
    async fn update_returns_rule_violations_when_language_unknown_and_runtime_negative() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(1, 1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .into_connection();
        let metadata = MovieMetadata { original_language: Some("xx".into()), runtime: Some(-1), ..movie_metadata(1) };

        let result = update(1, metadata, &fixtures::user(), &db).await;

        let violations = test_support::violations(result);
        assert_eq!(violations.len(), 2);
        assert!(violations.iter().any(|v| matches!(v, RuleViolation::MediaInvalidOriginalLanguage)));
        assert!(violations.iter().any(|v| matches!(v, RuleViolation::MovieNegativeRuntime)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn lock_sets_movie_lock() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(5, 1)]])
            .append_query_results([Vec::<movie_locks::Model>::new()])
            .append_query_results([[movie_locks::Model { runtime: true, ..fixtures::movie_locks_model(5) }]])
            .into_connection();

        let result = lock(5, "runtime".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "movie_locks""#, &[Value::from(5), Value::from(true)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "media_locks""#);
    }

    #[tokio::test]
    async fn lock_updates_existing_row_when_movie_already_has_locks() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(5, 1)]])
            .append_query_results([[movie_locks::Model { status: true, ..fixtures::movie_locks_model(5) }]])
            .append_query_results([[movie_locks::Model { status: true, runtime: true, ..fixtures::movie_locks_model(5) }]])
            .into_connection();

        let result = lock(5, "runtime".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "movie_locks""#, &[Value::from(true)]);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    #[tokio::test]
    async fn lock_sets_media_lock_when_property_belongs_to_media() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(5, 1)]])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[entities::media_locks::Model { overview: true, ..fixtures::media_locks_model(5) }]])
            .into_connection();

        let result = lock(5, "overview".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "media_locks""#, &[Value::from(5), Value::from(true)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "movie_locks""#);
    }

    #[tokio::test]
    async fn lock_returns_bad_request_when_property_unknown() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(5, 1)]])
            .into_connection();

        let result = lock(5, "colour".to_string(), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(_))));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn lock_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = lock(5, "runtime".to_string(), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn unlock_clears_movie_lock() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(5, 1)]])
            .append_query_results([[movie_locks::Model { runtime: true, ..fixtures::movie_locks_model(5) }]])
            .append_query_results([[fixtures::movie_locks_model(5)]])
            .into_connection();

        let result = unlock(5, "runtime".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "movie_locks""#, &[Value::from(false), Value::from(5)]);
    }

    /// Runs `movies::apply_changes` with a TMDB, store and jobs that must not be used.
    async fn run_apply_changes(media: &media::Model, changes: &PropertyChanges, db: &DbConn) -> Result<(), SrvErr> {
        apply_changes(media, changes, db, &MockTmdb::new(), &InMemoryImageStore::new(), &RecordingJobs::new(), &SeqRandom::new()).await
    }

    fn movie_changes() -> PropertyChanges {
        test_support::tmdb::property_changes(&[
            ("status", serde_json::json!("Ended")),
            ("runtime", serde_json::json!(120)),
            ("release_dates", serde_json::json!({"iso_3166_1": "US", "release_date": "2024-05-01T00:00:00.000Z"})),
        ])
    }

    fn us_movie() -> media::Model {
        media::Model { origin_country: Some(vec!["US".into()]), ..fixtures::movie_media_model(1, 1) }
    }

    #[tokio::test]
    async fn apply_changes_updates_unlocked_properties() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<movie_locks::Model>::new()])
            .append_query_results([[fixtures::movie_model(1)]])
            .into_connection();

        let result = run_apply_changes(&us_movie(), &movie_changes(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "movies""#, &[
            Value::from("Ended"), Value::from(120), Value::from(date(2024, 5, 1)), Value::from(1),
        ]);
        assert_no_statement_starting_with(&log, r#"UPDATE "media""#);
    }

    #[tokio::test]
    async fn apply_changes_skips_locked_properties() {
        test_support::init();
        let locks = movie_locks::Model { release_date: true, runtime: true, status: true, ..fixtures::movie_locks_model(1) };
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[locks]])
            .into_connection();

        let result = run_apply_changes(&us_movie(), &movie_changes(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_writes_only_unlocked_properties_when_some_locked() {
        test_support::init();
        let locks = movie_locks::Model { status: true, ..fixtures::movie_locks_model(1) };
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[locks]])
            .append_query_results([[fixtures::movie_model(1)]])
            .into_connection();

        let result = run_apply_changes(&us_movie(), &movie_changes(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "movies""#, &[Value::from(120), Value::from(date(2024, 5, 1))]);
        let update = log.iter().find(|s| s.sql.starts_with(r#"UPDATE "movies""#)).expect("movies update");
        let values = update.values.as_ref().expect("update values");
        assert!(!values.0.contains(&Value::from("Ended")), "the locked status must not be written");
    }

    #[tokio::test]
    async fn apply_changes_skips_release_date_when_country_not_an_origin_country() {
        test_support::init();
        let changes = test_support::tmdb::property_changes(&[
            ("release_dates", serde_json::json!({"iso_3166_1": "FR", "release_date": "2024-05-01"})),
        ]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<movie_locks::Model>::new()])
            .into_connection();

        let result = run_apply_changes(&us_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_applies_media_properties_when_changes_include_them() {
        test_support::init();
        let changes = test_support::tmdb::property_changes(&[
            ("homepage", serde_json::json!("https://moved.example.test")),
            ("status", serde_json::json!("Released")),
        ]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[fixtures::movie_media_model(1, 1)]])
            .append_query_results([Vec::<movie_locks::Model>::new()])
            .append_query_results([[fixtures::movie_model(1)]])
            .into_connection();

        let result = run_apply_changes(&us_movie(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from("https://moved.example.test")]);
        assert_wrote(&log, r#"UPDATE "movies""#, &[Value::from("Released")]);
    }
}