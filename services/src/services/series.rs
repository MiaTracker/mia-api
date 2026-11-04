use entities::prelude::{Episodes, ImageSizes, MediaLocks, Seasons, SeriesLocks};
use entities::prelude::Titles;
use entities::prelude::Languages;
use entities::prelude::Media;
use entities::prelude::Series;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, DatabaseTransaction, DbConn, ColumnTrait, EntityTrait, ModelTrait, NotSet, QueryFilter, TransactionTrait, PaginatorTrait, IntoActiveModel as SeaOrmIntoActiveModel, QueryOrder, QuerySelect};
use sea_orm::sea_query::OnConflict;
use sea_orm::prelude::Expr;
use sea_orm::FromQueryResult;
use std::collections::HashMap;
use views::images::Image;
use entities::{episodes, functions, genres, image_sizes, media, media_genres, seasons, series, series_locks, titles};
use integrations::tmdb::views::{SeasonDetails, SeriesTitle, TmdbEpisode};
use views::users::CurrentUser;
use crate::infrastructure::{constants, Clock, ImageStore, Random, RuleViolation, SrvErr, Tmdb};
use crate::jobs::Jobs;
use entities::sea_orm_active_enums::{ImageType, MediaType};
use entities::traits::linked::MediaPosters;
use chrono::NaiveDate;
use integrations::tmdb::views::PropertyChanges;
use views::media::{MediaIndex, PageReq};
use views::series::SeriesMetadata;
use entities::traits::locks::{SetLock, ToLocks};
use log::error;
use crate::infrastructure::traits::IntoActiveModel;
use crate::media::{fetch_media, try_set_media_lock};
use crate::services;
use crate::services::images::{fetch_backdrop_and_poster, save_tmdb_image};
use crate::services::media_details::{load_parts, MediaParts};

#[allow(clippy::too_many_arguments)]
pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, clock: &dyn Clock, random: &dyn Random) -> Result<(bool, i32), SrvErr> {
    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Series)).one(db).await;

    match med_res {
        Ok(media) => {
            if media.is_some() {
                return Ok((false, media.unwrap().id))
            }
        }
        Err(err) => { return Err(SrvErr::from(err)); }
    }

    // Pre-fetch all TMDB data and save images before opening the transaction so
    // that slow/failing HTTP calls cannot leave partial rows committed.
    let tmdb_series = match tmdb.series_details(tmdb_id).await {
        Ok(series) => series,
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let backdrop_image_id = match &tmdb_series.backdrop_path {
        None => None,
        Some(path) => Some(save_tmdb_image(path.as_str(), ImageType::Backdrop, db, tmdb, store, jobs, random).await?),
    };
    let poster_image_id = match &tmdb_series.poster_path {
        None => None,
        Some(path) => Some(save_tmdb_image(path.as_str(), ImageType::Poster, db, tmdb, store, jobs, random).await?),
    };

    let mut seasons_data: Vec<(SeasonDetails, Option<i32>, Vec<Option<i32>>)> = Vec::new();
    if let Some(seasons) = &tmdb_series.seasons {
        for season in seasons {
            if let Some(sn) = season.season_number {
                seasons_data.push(fetch_and_save_season(tmdb_id, sn, db, tmdb, store, jobs, random).await?);
            }
        }
    }

    let alt_titles = tmdb.series_alternative_titles(tmdb_id).await?;

    // All HTTP calls are done. Open a short transaction for DB-only inserts.
    let tran = db.begin().await?;

    let mut media: media::ActiveModel = tmdb_series.into_active_model();
    let mut series: series::ActiveModel = tmdb_series.into_active_model();

    media.backdrop_image_id = Set(backdrop_image_id);
    media.poster_image_id = Set(poster_image_id);
    media.date_added = Set(clock.now().date_naive());
    media.user_id = Set(user.id);
    media.bot_controllable = Set(user.though_bot);
    let inserted_media = media.insert(&tran).await?;
    series.id = Set(inserted_media.id);
    series.insert(&tran).await?;

    for (season_details, poster_id, episode_still_ids) in &seasons_data {
        insert_season_with_episodes(inserted_media.id, season_details, *poster_id, episode_still_ids, &tran).await?;
    }

    for genre in &tmdb_series.genres {
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id))
            .filter(genres::Column::Type.eq(MediaType::Series)).one(&tran).await?;
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
        title: Set(tmdb_series.name)
    };
    model.insert(&tran).await?;

    let filtered_titles = alt_titles.results.iter().filter(|x| {
        if x.iso_3166_1 == constants::TMDB_3166_1 { return true }
        if let Some(countries) = &tmdb_series.production_countries {
            return countries.iter().any(|y| { y.iso_3166_1 == x.iso_3166_1 })
        }
        true
    }).collect::<Vec<&SeriesTitle>>();
    for title in filtered_titles {
        let model = titles::ActiveModel {
            id: NotSet,
            media_id: Set(inserted_media.id),
            primary: Set(false),
            title: Set(title.title.clone())
        };
        model.insert(&tran).await?;
    }

    tran.commit().await?;
    Ok((true, inserted_media.id))
}

pub async fn index(page_req: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media = Media::find().filter(media::Column::UserId.eq(user.id))
        .find_also_linked(MediaPosters)
        .find_also_related(Titles)
        .filter(media::Column::Type.eq(MediaType::Series))
        .filter(titles::Column::Primary.eq(true)).order_by_asc(functions::default_media_sort())
        .offset(page_req.offset).limit(page_req.limit).all(db).await?;

    let poster_ids: Vec<i32> = media.iter().filter_map(|p| p.0.poster_image_id).collect();
    let poster_sizes = ImageSizes::find()
        .filter(image_sizes::Column::ImageId.is_in(poster_ids)).all(db).await?;

    let indexes = services::media::build_media_indexes(media, poster_sizes);

    Ok(indexes)
}

pub async fn details(series_id: i32, user: &CurrentUser, db: &DbConn) -> Result<Option<views::series::SeriesDetails>, SrvErr> {
    let db_media = Media::find().filter(media::Column::Id.eq(series_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Series)).one(db).await?;

    if db_media.is_none() {
        return Ok(None);
    }
    let db_media = db_media.unwrap();

    let db_series = load_series(&[db_media.id], db).await?.remove(&db_media.id).expect("DB in invalid state!");
    let parts = load_parts(std::slice::from_ref(&db_media), db).await?.remove(&db_media.id).expect("parts of every media");

    let media_locks = db_media.find_related(MediaLocks).one(db).await?;
    let series_locks = db_series.find_related(SeriesLocks).one(db).await?;
    let mut locks = media_locks.to_locks();
    locks.append(&mut series_locks.to_locks());

    let (backdrop, poster) = fetch_backdrop_and_poster(&db_media, db).await?;

    let counts = load_counts(&[db_series.id], db).await?[&db_series.id];

    Ok(Some(details_view(db_media, db_series, parts, counts, locks, backdrop, poster)))
}

/// The series rows of the media `media_ids`, keyed by id, in one query. The caller passes ids of
/// media already filtered by user and type; no ownership is checked here.
pub(crate) async fn load_series(media_ids: &[i32], db: &DbConn) -> Result<HashMap<i32, series::Model>, SrvErr> {
    let series = Series::find().filter(series::Column::Id.is_in(media_ids.to_vec())).all(db).await?;
    Ok(series.into_iter().map(|s| (s.id, s)).collect())
}

/// `(number_of_seasons, number_of_episodes)` of every series in `series_ids`, `(0, 0)` for one
/// without seasons. Two grouped queries whatever the number of series, none for an empty slice.
///
/// The caller passes ids of series already filtered by user; no ownership is checked here.
/// `pub` so the Postgres tests can check the grouped queries directly.
pub async fn load_counts(series_ids: &[i32], db: &DbConn) -> Result<HashMap<i32, (i32, i32)>, SrvErr> {
    #[derive(FromQueryResult)]
    struct SeriesCount {
        series_id: i32,
        count: i64,
    }

    if series_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let seasons = Seasons::find().filter(seasons::Column::SeriesId.is_in(series_ids.to_vec()))
        .select_only()
        .column(seasons::Column::SeriesId)
        .column_as(Expr::col((seasons::Entity, seasons::Column::Id)).count(), "count")
        .group_by(seasons::Column::SeriesId)
        .into_model::<SeriesCount>().all(db).await?;
    let episodes = Episodes::find().inner_join(seasons::Entity)
        .filter(seasons::Column::SeriesId.is_in(series_ids.to_vec()))
        .select_only()
        .column(seasons::Column::SeriesId)
        .column_as(Expr::col((episodes::Entity, episodes::Column::Id)).count(), "count")
        .group_by(seasons::Column::SeriesId)
        .into_model::<SeriesCount>().all(db).await?;

    let mut counts: HashMap<i32, (i32, i32)> = series_ids.iter().map(|id| (*id, (0, 0))).collect();
    for row in seasons {
        if let Some(c) = counts.get_mut(&row.series_id) {
            c.0 = row.count as i32;
        }
    }
    for row in episodes {
        if let Some(c) = counts.get_mut(&row.series_id) {
            c.1 = row.count as i32;
        }
    }
    Ok(counts)
}

/// The details of a series from its rows and its loaded parts, `counts` being
/// `(number_of_seasons, number_of_episodes)`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn details_view(media: media::Model, series: series::Model, parts: MediaParts, counts: (i32, i32), locks: Vec<&'static str>, backdrop: Option<Image>, poster: Option<Image>) -> views::series::SeriesDetails {
    views::series::SeriesDetails {
        id: media.id,
        poster,
        backdrop,
        stars: media.stars,
        title: parts.title,
        alternative_titles: parts.alternative_titles,
        first_air_date: series.first_air_date,
        number_of_episodes: Some(counts.1),
        number_of_seasons: Some(counts.0),
        status: series.status,
        r#type: series.r#type,
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

pub async fn metadata(series_id: i32, user: &CurrentUser, db: &DbConn) -> Result<SeriesMetadata, SrvErr> {
    let db_media = Media::find().filter(media::Column::Id.eq(series_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Series)).one(db).await?;

    if db_media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let db_media = db_media.unwrap();

    let series = db_media.find_related(Series).one(db).await?.expect("DB in invalid state!");
    let title = db_media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?.map(|m| { m.title });

    let metadata = SeriesMetadata {
        id: series.id,
        homepage: db_media.homepage,
        imdb_id: db_media.imdb_id,
        title,
        overview: db_media.overview,
        original_language: db_media.original_language,
        origin_country: db_media.origin_country,
        first_air_date: series.first_air_date,
        status: series.status,
        r#type: series.r#type,
    };
    Ok(metadata)
}

pub async fn update(series_id: i32, metadata: SeriesMetadata, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if series_id != metadata.id {
        return Err(SrvErr::BadRequest("Endpoint series id does not match series id in metadata!".to_string()));
    }

    let media = Media::find().filter(media::Column::Id.eq(series_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Series)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let mut rule_violations = Vec::new();

    if metadata.original_language != media.original_language && metadata.original_language.is_some() {
        let found = Languages::find_by_id(metadata.original_language.as_ref().unwrap()).count(db).await?;
        if found == 0 {
            rule_violations.push(RuleViolation::MediaInvalidOriginalLanguage);
        }
    }

    if !rule_violations.is_empty() {
        return Err(SrvErr::RuleViolation(rule_violations));
    }

    let tran = db.begin().await?;

    let mut media_am: media::ActiveModel = (&metadata).into_active_model();
    media_am.bot_controllable = sea_orm::Set(media.bot_controllable && user.though_bot);
    media_am.update(&tran).await?;
    let series_am: series::ActiveModel = (&metadata).into_active_model();
    series_am.update(&tran).await?;

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

pub async fn lock(series_id: i32, property: String, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    set_lock(series_id, property, true, user, db).await
}

pub async fn unlock(series_id: i32, property: String, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    set_lock(series_id, property, false, user, db).await
}

async fn set_lock(series_id: i32, property: String, locked: bool, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = fetch_media(series_id, views::media::MediaType::Series, user, db).await?;
    if try_set_media_lock(media, &property, locked, db).await? {
        return Ok(());
    }

    if !series_locks::ActiveModel::has_lock(&property) {
        return Err(SrvErr::BadRequest("Lock or property does not exist!".to_string()));
    }

    let locks = SeriesLocks::find().filter(series_locks::Column::SeriesId.eq(series_id)).one(db).await?;
    if let Some(locks) = locks {
        let mut active_locks = locks.into_active_model();
        active_locks.set_lock(&property, locked);
        active_locks.update(db).await?;
    }
    else {
        let mut active_locks = series_locks::ActiveModel {
            series_id: Set(series_id),
            first_air_date: Set(false),
            status: Set(false),
            r#type: Set(false),
        };
        active_locks.set_lock(&property, locked);
        active_locks.insert(db).await?;
    }
    Ok(())
}

/// Apply TMDB property-level changes to a series, respecting locks.
#[allow(clippy::too_many_arguments)]
pub async fn apply_changes(media: &entities::media::Model, changes: &PropertyChanges, start_date: NaiveDate, end_date: NaiveDate, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<(), SrvErr> {
    crate::media::apply_changes(media, changes, db, tmdb, store, jobs, random).await?;

    let series_locks = SeriesLocks::find()
        .filter(series_locks::Column::SeriesId.eq(media.id))
        .one(db)
        .await?;

    let mut series_am = series::ActiveModel {
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
                if !series_locks.as_ref().map_or(false, |l| l.status) {
                    series_am.status = Set(value.as_str().map(|s| s.to_string()));
                    any_change = true;
                }
            }
            "type" => {
                if !series_locks.as_ref().map_or(false, |l| l.r#type) {
                    series_am.r#type = Set(value.as_str().map(|s| s.to_string()));
                    any_change = true;
                }
            }
            "first_air_date" => {
                if !series_locks.as_ref().map_or(false, |l| l.first_air_date) {
                    if let Some(date_str) = value.as_str() {
                        let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok();
                        if let Some(date) = date {
                            series_am.first_air_date = Set(Some(date));
                            any_change = true;
                        }
                    }
                }
            }
            "season" => {
                let season_tmdb_id = value.get("season_id").and_then(|v| v.as_i64()).map(|n| n as i32);
                let season_number = value.get("season_number").and_then(|v| v.as_i64()).map(|n| n as i32);

                if let (Some(stid), Some(sn)) = (season_tmdb_id, season_number) {
                    if last_item.action == "deleted" {
                        seasons::Entity::delete_many()
                            .filter(seasons::Column::SeriesId.eq(media.id))
                            .filter(seasons::Column::SeasonNumber.eq(sn))
                            .exec(db).await?;
                        continue;
                    }

                    let season_opt = seasons::Entity::find()
                        .filter(seasons::Column::SeriesId.eq(media.id))
                        .filter(seasons::Column::SeasonNumber.eq(sn))
                        .one(db).await?;

                    if last_item.action == "added" || season_opt.is_none() {
                        if let Some(series_tmdb_id) = media.tmdb_id {
                            if let Err(e) = upsert_season_with_episodes(series_tmdb_id, media.id, sn, db, tmdb, store, jobs, random).await {
                                error!("Failed to create new season series_id={} season={}: {}", media.id, sn, e);
                            }
                        }
                    } else if let Some(season) = season_opt {
                        match tmdb.season_property_changes(stid, start_date, end_date).await {
                            Err(e) => error!("Failed season property_changes series_id={} season={}: {}", media.id, sn, e),
                            Ok(sc) => {
                                apply_season_property_changes(&season, &sc, db, tmdb, store, jobs, random).await?;

                                for ep_change in &sc.changes {
                                    if ep_change.key != "episode" { continue; }
                                    let last_ep_item = match ep_change.items.last() { Some(i) => i, None => continue };

                                    let ep_tmdb_id = last_ep_item.value.as_ref()
                                        .and_then(|v| v.get("episode_id"))
                                        .and_then(|v| v.as_i64())
                                        .map(|n| n as i32);

                                    if let Some(etid) = ep_tmdb_id {
                                        if last_ep_item.action == "deleted" {
                                            episodes::Entity::delete_many()
                                                .filter(episodes::Column::SeasonId.eq(season.id))
                                                .filter(episodes::Column::TmdbId.eq(etid))
                                                .exec(db).await?;
                                            continue;
                                        }

                                        let episode = episodes::Entity::find()
                                            .filter(episodes::Column::SeasonId.eq(season.id))
                                            .filter(episodes::Column::TmdbId.eq(etid))
                                            .one(db).await?;

                                        if last_ep_item.action == "added" || episode.is_none() {
                                            if let Some(series_tmdb_id) = media.tmdb_id {
                                                if let Err(e) = upsert_season_with_episodes(series_tmdb_id, media.id, sn, db, tmdb, store, jobs, random).await {
                                                    error!("Failed to create new episode series_id={} season={}: {}", media.id, sn, e);
                                                }
                                            }
                                        } else if let Some(ep) = episode {
                                            match tmdb.episode_property_changes(etid, start_date, end_date).await {
                                                Err(e) => error!("Failed episode property_changes ep_tmdb_id={}: {}", etid, e),
                                                Ok(ec) => {
                                                    if let Err(e) = apply_episode_changes(&ep, &ec, db, tmdb, store, jobs, random).await {
                                                        error!("Failed to apply episode changes ep_tmdb_id={}: {}", etid, e);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if any_change {
        series_am.update(db).await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn upsert_season_with_episodes(
    series_tmdb_id: i32,
    series_db_id: i32,
    season_number: i32,
    db: &DbConn,
    tmdb: &dyn Tmdb,
    store: &dyn ImageStore,
    jobs: &dyn Jobs,
    random: &dyn Random,
) -> Result<seasons::Model, SrvErr> {
    let season_details = tmdb.season_details(series_tmdb_id, season_number).await?;

    let existing = seasons::Entity::find()
        .filter(seasons::Column::SeriesId.eq(series_db_id))
        .filter(seasons::Column::SeasonNumber.eq(season_number))
        .one(db)
        .await?;

    let season_model = if let Some(existing_season) = existing {
        let needs_poster = existing_season.poster_image_id.is_none();
        let mut am = existing_season.into_active_model();
        am.air_date = Set(season_details.air_date.as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()));
        am.episode_count = Set(Some(season_details.episodes.len() as i32));
        am.name = Set(season_details.name.clone());
        am.overview = Set(season_details.overview.clone());
        if needs_poster {
            if let Some(path) = &season_details.poster_path {
                if !path.is_empty() {
                    am.poster_image_id = Set(Some(save_tmdb_image(path, ImageType::Poster, db, tmdb, store, jobs, random).await?));
                }
            }
        }
        am.tmdb_vote_average = Set(season_details.vote_average);
        am.tmdb_id = Set(Some(season_details.id));
        am.stars = NotSet;
        am.update(db).await?
    } else {
        let mut am = <&SeasonDetails as IntoActiveModel<seasons::ActiveModel>>::into_active_model(&season_details);
        am.series_id = Set(series_db_id);
        if let Some(path) = &season_details.poster_path {
            if !path.is_empty() {
                am.poster_image_id = Set(Some(save_tmdb_image(path, ImageType::Poster, db, tmdb, store, jobs, random).await?));
            }
        }
        am.insert(db).await?
    };

    for episode in &season_details.episodes {
        let mut ep_am = <&TmdbEpisode as IntoActiveModel<episodes::ActiveModel>>::into_active_model(episode);
        ep_am.season_id = Set(season_model.id);
        let still_image_id = if let Some(path) = &episode.still_path {
            if !path.is_empty() {
                save_tmdb_image(path, ImageType::Still, db, tmdb, store, jobs, random).await.ok()
            } else {
                None
            }
        } else {
            None
        };
        ep_am.still_image_id = Set(still_image_id);
        episodes::Entity::insert(ep_am)
            .on_conflict(
                OnConflict::columns([episodes::Column::SeasonId, episodes::Column::TmdbId])
                    .update_columns([
                        episodes::Column::EpisodeNumber,
                        episodes::Column::Name,
                        episodes::Column::Overview,
                        episodes::Column::AirDate,
                        episodes::Column::Runtime,
                        episodes::Column::TmdbVoteAverage,
                        episodes::Column::StillImageId,
                    ])
                    .to_owned(),
            )
            .exec(db)
            .await?;
    }

    Ok(season_model)
}

async fn fetch_and_save_season(
    series_tmdb_id: i32,
    season_number: i32,
    db: &DbConn,
    tmdb: &dyn Tmdb,
    store: &dyn ImageStore,
    jobs: &dyn Jobs,
    random: &dyn Random,
) -> Result<(SeasonDetails, Option<i32>, Vec<Option<i32>>), SrvErr> {
    let season_details = tmdb.season_details(series_tmdb_id, season_number).await?;

    let poster_image_id = if let Some(path) = &season_details.poster_path {
        if !path.is_empty() { save_tmdb_image(path, ImageType::Poster, db, tmdb, store, jobs, random).await.ok() } else { None }
    } else {
        None
    };

    let mut episode_still_ids = Vec::with_capacity(season_details.episodes.len());
    for episode in &season_details.episodes {
        let still_id = if let Some(path) = &episode.still_path {
            if !path.is_empty() { save_tmdb_image(path, ImageType::Still, db, tmdb, store, jobs, random).await.ok() } else { None }
        } else {
            None
        };
        episode_still_ids.push(still_id);
    }

    Ok((season_details, poster_image_id, episode_still_ids))
}

async fn insert_season_with_episodes(
    series_db_id: i32,
    season_details: &SeasonDetails,
    poster_image_id: Option<i32>,
    episode_still_ids: &[Option<i32>],
    tran: &DatabaseTransaction,
) -> Result<(), SrvErr> {
    let mut am = <&SeasonDetails as IntoActiveModel<seasons::ActiveModel>>::into_active_model(season_details);
    am.series_id = Set(series_db_id);
    am.poster_image_id = Set(poster_image_id);
    let season_model = am.insert(tran).await?;

    for (episode, still_image_id) in season_details.episodes.iter().zip(episode_still_ids.iter()) {
        let mut ep_am = <&TmdbEpisode as IntoActiveModel<episodes::ActiveModel>>::into_active_model(episode);
        ep_am.season_id = Set(season_model.id);
        ep_am.still_image_id = Set(*still_image_id);
        episodes::Entity::insert(ep_am)
            .on_conflict(
                OnConflict::columns([episodes::Column::SeasonId, episodes::Column::TmdbId])
                    .update_columns([
                        episodes::Column::EpisodeNumber,
                        episodes::Column::Name,
                        episodes::Column::Overview,
                        episodes::Column::AirDate,
                        episodes::Column::Runtime,
                        episodes::Column::TmdbVoteAverage,
                        episodes::Column::StillImageId,
                    ])
                    .to_owned(),
            )
            .exec(tran)
            .await?;
    }

    Ok(())
}

async fn apply_season_property_changes(
    season: &seasons::Model,
    changes: &PropertyChanges,
    db: &DbConn,
    tmdb: &dyn Tmdb,
    store: &dyn ImageStore,
    jobs: &dyn Jobs,
    random: &dyn Random,
) -> Result<(), SrvErr> {
    let mut am = seasons::ActiveModel {
        id: Set(season.id),
        ..Default::default()
    };
    let mut any_change = false;

    for change in &changes.changes {
        let last_item = match change.items.last() { Some(i) => i, None => continue };
        let value = match &last_item.value { Some(v) => v, None => continue };

        match change.key.as_str() {
            "air_date" => {
                if let Some(date_str) = value.as_str() {
                    am.air_date = Set(NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok());
                    any_change = true;
                }
            }
            "name" => {
                if let Some(s) = value.as_str() {
                    am.name = Set(Some(s.to_string()));
                    any_change = true;
                }
            }
            "overview" => {
                if let Some(s) = value.as_str() {
                    am.overview = Set(Some(s.to_string()));
                    any_change = true;
                }
            }
            "poster_path" => {
                if season.poster_image_id.is_none() {
                    if let Some(s) = value.as_str() {
                        let id = save_tmdb_image(s, ImageType::Poster, db, tmdb, store, jobs, random).await?;
                        am.poster_image_id = Set(Some(id));
                        any_change = true;
                    }
                }
            }
            _ => {}
        }
    }

    if any_change {
        am.update(db).await?;
    }

    Ok(())
}

async fn apply_episode_changes(
    episode: &episodes::Model,
    changes: &PropertyChanges,
    db: &DbConn,
    tmdb: &dyn Tmdb,
    store: &dyn ImageStore,
    jobs: &dyn Jobs,
    random: &dyn Random,
) -> Result<(), SrvErr> {
    let mut am = episodes::ActiveModel {
        id: Set(episode.id),
        ..Default::default()
    };
    let mut any_change = false;

    for change in &changes.changes {
        let last_item = match change.items.last() { Some(i) => i, None => continue };
        let value = match &last_item.value { Some(v) => v, None => continue };

        match change.key.as_str() {
            "name" => {
                if let Some(s) = value.as_str() {
                    am.name = Set(Some(s.to_string()));
                    any_change = true;
                }
            }
            "overview" => {
                if let Some(s) = value.as_str() {
                    am.overview = Set(Some(s.to_string()));
                    any_change = true;
                }
            }
            "air_date" => {
                if let Some(date_str) = value.as_str() {
                    am.air_date = Set(NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok());
                    any_change = true;
                }
            }
            "runtime" => {
                if let Some(n) = value.as_i64() {
                    am.runtime = Set(Some(n as i32));
                    any_change = true;
                }
            }
            "still_path" => {
                if episode.still_image_id.is_none() {
                    if let Some(s) = value.as_str() {
                        let id = save_tmdb_image(s, ImageType::Still, db, tmdb, store, jobs, random).await?;
                        am.still_image_id = Set(Some(id));
                        any_change = true;
                    }
                }
            }
            _ => {}
        }
    }

    if any_change {
        am.update(db).await?;
    }

    Ok(())
}
#[cfg(test)]
mod tests {
    use entities::images;
    use integrations::tmdb::views::{Genre as TmdbGenre, ProductionCountry, Season, SeriesDetails as TmdbSeriesDetails, SeriesTitles};
    use sea_orm::Value;
    use uuid::Uuid;
    use super::*;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted, with_saved_tmdb_image};
    use crate::test_support::fixtures;
    use crate::test_support::jobs::sizes;
    use crate::test_support::tmdb::{png_bytes, property_changes, season_details, tmdb_episode};
    use crate::test_support::{date, fixed_clock, InMemoryImageStore, MockTmdb, RecordingJobs, SeqRandom};

    fn series_genre(id: i32, tmdb_id: i32, name: &str) -> genres::Model {
        genres::Model { tmdb_id: Some(tmdb_id), ..fixtures::genre_model(id, name, MediaType::Series) }
    }

    /// Runs `create` with a fixed clock and sequential randomness.
    async fn run_create(tmdb_id: i32, user: &CurrentUser, db: &DbConn, tmdb: &MockTmdb, store: &InMemoryImageStore, jobs: &RecordingJobs) -> Result<(bool, i32), SrvErr> {
        create(tmdb_id, user, db, tmdb, store, jobs, &fixed_clock(2026, 3, 4, 12, 0, 0), &SeqRandom::new()).await
    }

    /// A series without images, genres or seasons.
    fn bare_series(id: i32) -> TmdbSeriesDetails {
        TmdbSeriesDetails { backdrop_path: None, poster_path: None, ..test_support::tmdb::series_details(id) }
    }

    /// A series without images or genres whose seasons are numbered `season_numbers`.
    fn series_with_seasons(id: i32, season_numbers: &[i32]) -> TmdbSeriesDetails {
        TmdbSeriesDetails {
            seasons: Some(season_numbers.iter().map(|n| Season {
                id: id * 100 + n, season_number: Some(*n), ..Season::default()
            }).collect()),
            ..bare_series(id)
        }
    }

    /// A TMDB answering `series_details` with a series without images or genres, and no alternative titles.
    fn bare_series_tmdb() -> MockTmdb {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().times(1).returning(|id| Ok(bare_series(id)));
        tmdb.expect_series_alternative_titles().times(1).returning(|id| Ok(SeriesTitles { id, results: Vec::new() }));
        tmdb
    }

    #[tokio::test]
    async fn create_returns_existing_id_when_series_already_added() {
        test_support::init();
        let tmdb = MockTmdb::new();
        let db = test_support::db::mock()
            .append_query_results([[media::Model { tmdb_id: Some(70), ..fixtures::series_media_model(5, 1) }]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        assert!(matches!(result, Ok((false, 5))));
        assert!(jobs.take().is_empty());
        assert!(store.files().is_empty());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(70), Value::from(1)]);
    }

    #[tokio::test]
    async fn create_inserts_media_series_titles_and_genres() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().withf(|id| *id == 70).times(1).returning(|id| Ok(TmdbSeriesDetails {
            genres: vec![TmdbGenre { id: 18, name: "Drama".into() }, TmdbGenre { id: 80, name: "Crime".into() }],
            ..test_support::tmdb::series_details(id)
        }));
        tmdb.expect_series_alternative_titles().withf(|id| *id == 70).times(1).returning(|id| Ok(SeriesTitles {
            id,
            results: vec![SeriesTitle { iso_3166_1: "DE".into(), title: "Serie Deutsch".into(), r#type: String::new() }],
        }));
        tmdb.expect_image().withf(|path| path == "/series-70-backdrop.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        tmdb.expect_image().withf(|path| path == "/series-70-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()]);
        let db = with_saved_tmdb_image(db, 11, ImageType::Backdrop);
        let db = with_saved_tmdb_image(db, 12, ImageType::Poster)
            // media, series, then per genre: lookup and link; then titles
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([[series_genre(5, 18, "Drama")]])
            .append_query_results([[fixtures::media_genre_model(10, 5)]])
            .append_query_results([[series_genre(6, 80, "Crime")]])
            .append_query_results([[fixtures::media_genre_model(10, 6)]])
            .append_query_results([[fixtures::title_model(1, 10, "Series 70", true)]])
            .append_query_results([[fixtures::title_model(2, 10, "Serie Deutsch", false)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        // The clock's date, the user, and not bot controllable; the saved images' ids.
        assert_wrote(&log, r#"INSERT INTO "media""#, &[
            Value::from(date(2026, 3, 4)), Value::from(1), Value::from(false),
            Value::from(70), Value::from(11), Value::from(12),
        ]);
        assert_wrote(&log, r#"INSERT INTO "series""#, &[
            Value::from(10), Value::from(date(2024, 1, 1)), Value::from("Ended"), Value::from("Scripted"),
        ]);
        assert_wrote(&log, r#"INSERT INTO "media_genres""#, &[Value::from(10), Value::from(5)]);
        assert_wrote(&log, r#"INSERT INTO "media_genres""#, &[Value::from(10), Value::from(6)]);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(10), Value::from("Series 70"), Value::from(true)]);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(10), Value::from("Serie Deutsch"), Value::from(false)]);
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/series-70-backdrop.jpg")]);
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/series-70-poster.jpg")]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "seasons""#);
        assert_wrote(&log, "COMMIT", &[]);
        assert_eq!(store.files(), vec![
            (Uuid::from_u128(1000).simple().to_string(), "300x450.png".to_string()),
            (Uuid::from_u128(1001).simple().to_string(), "300x450.png".to_string()),
        ]);
        // One extra size per image; the jobs are not run here (covered by the job handler tests).
        assert_eq!(sizes(&jobs.take()), vec![(11, 100, 150), (12, 100, 150)]);
    }

    #[tokio::test]
    async fn create_inserts_seasons_and_episodes_from_season_details() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().withf(|id| *id == 70).times(1).returning(|id| Ok(series_with_seasons(id, &[1, 2])));
        for season in [1, 2] {
            tmdb.expect_season_details().withf(move |series_id, n| *series_id == 70 && *n == season).times(1)
                .returning(|series_id, n| Ok(SeasonDetails {
                    episodes: vec![tmdb_episode(n, 1), tmdb_episode(n, 2)],
                    ..season_details(series_id, n)
                }));
        }
        tmdb.expect_series_alternative_titles().times(1).returning(|id| Ok(SeriesTitles { id, results: Vec::new() }));
        tmdb.expect_image().times(6).returning(|_| Ok(png_bytes(300, 450)));
        // Images are saved in source order: for each season its poster, then its episodes' stills.
        let db = test_support::db::mock().append_query_results([Vec::<media::Model>::new()]);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster);
        let db = with_saved_tmdb_image(db, 22, ImageType::Still);
        let db = with_saved_tmdb_image(db, 23, ImageType::Still);
        let db = with_saved_tmdb_image(db, 24, ImageType::Poster);
        let db = with_saved_tmdb_image(db, 25, ImageType::Still);
        let db = with_saved_tmdb_image(db, 26, ImageType::Still)
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]])
            .append_query_results([[fixtures::episode_model(42, 31, 1002, 2)]])
            .append_query_results([[fixtures::season_model(32, 10, 2)]])
            .append_query_results([[fixtures::episode_model(43, 32, 2001, 1)]])
            .append_query_results([[fixtures::episode_model(44, 32, 2002, 2)]])
            .append_query_results([[fixtures::title_model(1, 10, "Series 70", true)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        // Each season: the series, its name, the saved poster and its TMDB id.
        assert_wrote(&log, r#"INSERT INTO "seasons""#, &[
            Value::from(10), Value::from("Season 1"), Value::from(21), Value::from(7001),
        ]);
        assert_wrote(&log, r#"INSERT INTO "seasons""#, &[
            Value::from(10), Value::from("Season 2"), Value::from(24), Value::from(7002),
        ]);
        // Each episode: its season, its TMDB id, the saved still.
        for (season_id, tmdb_id, still_id) in [(31, 1001, 22), (31, 1002, 23), (32, 2001, 25), (32, 2002, 26)] {
            assert_wrote(&log, r#"INSERT INTO "episodes""#, &[
                Value::from(season_id), Value::from(tmdb_id), Value::from(still_id),
            ]);
        }
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/series-70-season-1.jpg")]);
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/season-2-episode-2.jpg")]);
        assert_wrote(&log, "COMMIT", &[]);
        assert_eq!(store.files().len(), 6);
        assert_eq!(sizes(&jobs.take()), vec![
            (21, 100, 150), (22, 100, 150), (23, 100, 150), (24, 100, 150), (25, 100, 150), (26, 100, 150),
        ]);
    }

    #[tokio::test]
    async fn create_inserts_season_without_poster_when_poster_download_fails() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().times(1).returning(|id| Ok(series_with_seasons(id, &[1])));
        tmdb.expect_season_details().times(1).returning(|series_id, n| Ok(SeasonDetails {
            episodes: vec![TmdbEpisode { still_path: None, ..tmdb_episode(n, 1) }],
            ..season_details(series_id, n)
        }));
        tmdb.expect_series_alternative_titles().times(1).returning(|id| Ok(SeriesTitles { id, results: Vec::new() }));
        tmdb.expect_image().times(1).returning(|_| Err(test_support::tmdb::error("image unavailable")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([Vec::<images::Model>::new()])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]])
            .append_query_results([[fixtures::title_model(1, 10, "Series 70", true)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        // A season image that cannot be downloaded is skipped; the season and episode still land.
        assert!(matches!(result, Ok((true, 10))));
        assert!(store.files().is_empty());
        assert!(jobs.take().is_empty());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "seasons""#, &[Value::from(10), Value::from("Season 1")]);
        assert_wrote(&log, r#"INSERT INTO "episodes""#, &[Value::from(31), Value::from(1001)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "images""#);
        assert_wrote(&log, "COMMIT", &[]);
    }

    #[tokio::test]
    async fn create_keeps_only_alternative_titles_from_production_countries_when_listed() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().times(1).returning(|id| Ok(TmdbSeriesDetails {
            production_countries: Some(vec![ProductionCountry { iso_3166_1: "FR".into(), name: "France".into() }]),
            ..bare_series(id)
        }));
        tmdb.expect_series_alternative_titles().times(1).returning(|id| Ok(SeriesTitles {
            id,
            results: ["US", "FR", "DE"].iter().map(|country| SeriesTitle {
                iso_3166_1: country.to_string(), title: format!("Title {country}"), r#type: String::new(),
            }).collect(),
        }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([[fixtures::title_model(1, 10, "Series 70", true)]])
            .append_query_results([[fixtures::title_model(2, 10, "Title US", false)]])
            .append_query_results([[fixtures::title_model(3, 10, "Title FR", false)]])
            .into_connection();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from("Title US"), Value::from(false)]);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from("Title FR"), Value::from(false)]);
        assert!(
            !log.iter().any(|s| s.values.as_ref().is_some_and(|v| v.0.contains(&Value::from("Title DE")))),
            "a title from a country outside the production countries must not be written"
        );
    }

    #[tokio::test]
    async fn create_marks_media_bot_controllable_when_created_through_bot() {
        test_support::init();
        let tmdb = bare_series_tmdb();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([[fixtures::title_model(1, 10, "Series 70", true)]])
            .into_connection();

        let result = run_create(70, &fixtures::bot_user(), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(matches!(result, Ok((true, 10))));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "media""#, &[Value::from(true), Value::from(1)]);
    }

    #[tokio::test]
    async fn create_returns_masterdata_outdated_when_genre_unknown() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().times(1).returning(|id| Ok(TmdbSeriesDetails {
            genres: vec![TmdbGenre { id: 18, name: "Drama".into() }],
            ..bare_series(id)
        }));
        tmdb.expect_series_alternative_titles().times(1).returning(|id| Ok(SeriesTitles { id, results: Vec::new() }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([Vec::<genres::Model>::new()])
            .into_connection();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

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
        tmdb.expect_series_details().times(1).returning(|_| Err(test_support::tmdb::error("TMDB unavailable")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &store, &jobs).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
        assert!(store.files().is_empty());
        assert!(jobs.take().is_empty());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    #[tokio::test]
    async fn create_returns_err_and_writes_nothing_when_season_details_fail() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().times(1).returning(|id| Ok(series_with_seasons(id, &[1])));
        tmdb.expect_season_details().times(1).returning(|_, _| Err(test_support::tmdb::error("TMDB unavailable")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();
        let jobs = RecordingJobs::new();

        let result = run_create(70, &fixtures::user(), &db, &tmdb, &InMemoryImageStore::new(), &jobs).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
        assert!(jobs.take().is_empty());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    #[tokio::test]
    async fn index_returns_page_of_series_indexes() {
        test_support::init();
        let with_poster = media::Model { poster_image_id: Some(2), stars: Some(4.5), ..fixtures::series_media_model(1, 1) };
        let without_poster = fixtures::series_media_model(3, 1);
        let db = test_support::db::mock()
            .append_query_results([vec![
                fixtures::media_index_row(with_poster, Some(fixtures::image_model(2)), Some(fixtures::title_model(1, 1, "Andor", true))),
                fixtures::media_index_row(without_poster, None, Some(fixtures::title_model(3, 3, "Bluey", true))),
            ]])
            .append_query_results([[fixtures::image_size_model(1, 2, 300, 450), fixtures::image_size_model(2, 2, 100, 150)]])
            .into_connection();

        let page = PageReq { offset: Some(20), limit: Some(10) };
        let result = index(page, &fixtures::user(), &db).await.ok().expect("index should succeed");

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, 1);
        assert_eq!(result[0].title, "Andor");
        assert_eq!(result[0].stars, Some(4.5));
        let poster = result[0].poster.as_ref().expect("first series has a poster");
        assert_eq!(poster.path, "/image-2.jpg");
        assert_eq!(poster.sizes.len(), 2);
        assert_eq!(result[1].id, 3);
        assert_eq!(result[1].title, "Bluey");
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

        let result = details(7, &fixtures::user(), &db).await.ok().expect("details should succeed");

        assert!(result.is_none());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(7), Value::from(1)]);
    }

    #[tokio::test]
    async fn details_assembles_titles_genres_tags_sources_logs_images_and_season_counts() {
        test_support::init();
        let media = media::Model {
            tmdb_id: Some(70),
            overview: Some("A rebel spy.".into()),
            tmdb_vote_average: Some(8.2),
            original_language: Some("en".into()),
            stars: Some(4.5),
            poster_image_id: Some(2),
            ..fixtures::series_media_model(1, 1)
        };
        let log_row = entities::logs::Model { stars: Some(4.5), comment: Some("Great".into()), ..fixtures::log_model(1, 1, 9) };
        let db = test_support::db::mock()
            .append_query_results([[media]])
            .append_query_results([[fixtures::series_model(1)]])
            .append_query_results([[
                fixtures::title_model(1, 1, "Andor", true),
                fixtures::title_model(2, 1, "Star Wars: Andor", false),
            ]])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([[(fixtures::media_genre_model(1, 3), Some(fixtures::genre_model(3, "Drama", MediaType::Series)))]])
            .append_query_results([[(fixtures::media_tag_model(1, 4), Some(fixtures::tag_model(4, "space")))]])
            .append_query_results([[fixtures::source_model(9, 1, "Disney+")]])
            .append_query_results([[log_row]])
            .append_query_results([[fixtures::watchlist_model(1)]])
            .append_query_results([[entities::media_locks::Model { overview: true, ..fixtures::media_locks_model(1) }]])
            .append_query_results([[series_locks::Model { status: true, ..fixtures::series_locks_model(1) }]])
            .append_query_results([[(fixtures::image_model(2), Some(fixtures::image_size_model(1, 2, 300, 450)))]])
            .append_query_results([[fixtures::series_count_row(1, 2)]])
            .append_query_results([[fixtures::series_count_row(1, 24)]])
            .into_connection();

        let series = details(1, &fixtures::user(), &db).await.ok().expect("details should succeed")
            .expect("series should be found");

        assert_eq!(series.id, 1);
        assert_eq!(series.title, "Andor");
        assert_eq!(series.alternative_titles.len(), 1);
        assert_eq!(series.alternative_titles[0].id, 2);
        assert_eq!(series.alternative_titles[0].title, "Star Wars: Andor");
        assert_eq!(series.tmdb_id, Some(70));
        assert_eq!(series.overview.as_deref(), Some("A rebel spy."));
        assert_eq!(series.stars, Some(4.5));
        assert_eq!(series.first_air_date, Some(date(2024, 1, 1)));
        assert_eq!(series.status.as_deref(), Some("Ended"));
        assert_eq!(series.r#type.as_deref(), Some("Scripted"));
        assert_eq!(series.number_of_seasons, Some(2));
        assert_eq!(series.number_of_episodes, Some(24));
        assert!(series.on_watchlist);
        assert_eq!(series.original_language.as_ref().map(|l| l.english_name.as_str()), Some("English"));
        assert_eq!(series.genres.len(), 1);
        assert_eq!(series.genres[0].name, "Drama");
        assert_eq!(series.tags.len(), 1);
        assert_eq!(series.tags[0].name, "space");
        assert_eq!(series.sources.len(), 1);
        assert_eq!(series.sources[0].name, "Disney+");
        assert_eq!(series.logs.len(), 1);
        assert_eq!(series.logs[0].source, "Disney+");
        assert_eq!(series.logs[0].stars, Some(4.5));
        assert_eq!(series.logs[0].comment.as_deref(), Some("Great"));
        assert_eq!(series.locks, vec!["overview", "status"]);
        let poster = series.poster.expect("series has a poster");
        assert_eq!(poster.path, "/image-2.jpg");
        assert_eq!(poster.sizes.len(), 1);
        assert!(series.backdrop.is_none());
    }

    #[tokio::test]
    async fn details_uses_unset_title_when_no_primary_title() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(1, 1)]])
            .append_query_results([[fixtures::series_model(1)]])
            .append_query_results([[fixtures::title_model(2, 1, "Only Alternative", false)]])
            .append_query_results([Vec::<(media_genres::Model, Option<genres::Model>)>::new()])
            .append_query_results([Vec::<(entities::media_tags::Model, Option<entities::tags::Model>)>::new()])
            .append_query_results([Vec::<entities::sources::Model>::new()])
            .append_query_results([Vec::<entities::logs::Model>::new()])
            .append_query_results([Vec::<entities::watchlist::Model>::new()])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([Vec::<(images::Model, Option<image_sizes::Model>)>::new()])
            .append_query_results([fixtures::no_series_count_rows()])
            .append_query_results([fixtures::no_series_count_rows()])
            .into_connection();

        let series = details(1, &fixtures::user(), &db).await.ok().expect("details should succeed")
            .expect("series should be found");

        assert_eq!(series.title, "Unknown");
        assert_eq!(series.alternative_titles.len(), 1);
        assert!(!series.on_watchlist);
        assert!(series.original_language.is_none());
        assert!(series.locks.is_empty());
        assert!(series.poster.is_none());
        assert!(series.backdrop.is_none());
        assert_eq!(series.number_of_seasons, Some(0));
        assert_eq!(series.number_of_episodes, Some(0));
    }

    #[tokio::test]
    async fn load_counts_returns_seasons_and_episodes_per_series() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_count_row(1, 2)]])
            .append_query_results([[fixtures::series_count_row(1, 24)]])
            .into_connection();

        let counts = load_counts(&[1, 2], &db).await.ok().expect("load_counts should succeed");

        assert_eq!(counts.len(), 2);
        assert_eq!(counts[&1], (2, 24));
        assert_eq!(counts[&2], (0, 0), "a series without seasons");
        assert_statement_count(&test_support::db::log(db), 2);
    }

    #[tokio::test]
    async fn metadata_returns_current_values_without_reading_locks() {
        test_support::init();
        let media = media::Model {
            homepage: Some("https://andor.example.test".into()),
            imdb_id: Some("tt9253284".into()),
            overview: Some("A rebel spy.".into()),
            original_language: Some("en".into()),
            origin_country: Some(vec!["US".into()]),
            ..fixtures::series_media_model(1, 1)
        };
        let db = test_support::db::mock()
            .append_query_results([[media]])
            .append_query_results([[fixtures::series_model(1)]])
            .append_query_results([[fixtures::title_model(1, 1, "Andor", true)]])
            .into_connection();

        let metadata = metadata(1, &fixtures::user(), &db).await.ok().expect("metadata should succeed");

        assert_eq!(metadata.id, 1);
        assert_eq!(metadata.title.as_deref(), Some("Andor"));
        assert_eq!(metadata.homepage.as_deref(), Some("https://andor.example.test"));
        assert_eq!(metadata.imdb_id.as_deref(), Some("tt9253284"));
        assert_eq!(metadata.overview.as_deref(), Some("A rebel spy."));
        assert_eq!(metadata.original_language.as_deref(), Some("en"));
        assert_eq!(metadata.origin_country, Some(vec!["US".to_string()]));
        assert_eq!(metadata.first_air_date, Some(date(2024, 1, 1)));
        assert_eq!(metadata.status.as_deref(), Some("Ended"));
        assert_eq!(metadata.r#type.as_deref(), Some("Scripted"));
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

    fn series_metadata(id: i32) -> SeriesMetadata {
        SeriesMetadata {
            id,
            homepage: Some("https://new.example.test".into()),
            imdb_id: Some("tt0000001".into()),
            title: Some("New Title".into()),
            overview: Some("New overview".into()),
            original_language: None,
            origin_country: Some(vec!["CA".into()]),
            first_air_date: Some(date(2025, 6, 7)),
            status: Some("returning series".into()),
            r#type: Some("miniseries".into()),
        }
    }

    #[tokio::test]
    async fn update_writes_changed_fields_without_touching_locks() {
        test_support::init();
        let media = media::Model { bot_controllable: true, ..fixtures::series_media_model(1, 1) };
        let db = test_support::db::mock()
            .append_query_results([[media.clone()]])
            .append_query_results([[media]])
            .append_query_results([[fixtures::series_model(1)]])
            .append_query_results([[fixtures::title_model(1, 1, "Old Title", true)]])
            .append_query_results([[fixtures::title_model(1, 1, "New Title", true)]])
            .into_connection();

        let result = update(1, series_metadata(1), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[
            Value::from("https://new.example.test"), Value::from("tt0000001"), Value::from("New overview"),
            // A user who is not a bot takes over the media from the bot.
            Value::from(false),
        ]);
        assert_wrote(&log, r#"UPDATE "series""#, &[
            Value::from(date(2025, 6, 7)), Value::from("Returning Series"), Value::from("Miniseries"),
        ]);
        assert_wrote(&log, r#"UPDATE "titles""#, &[Value::from("New Title")]);
        // Editing does not touch the lock tables; locks are set through `lock`.
        assert_no_statement_starting_with(&log, "INSERT");
        assert!(log.iter().all(|s| !s.sql.contains("_locks")), "update should not read or write locks");
    }

    #[tokio::test]
    async fn update_keeps_bot_controllable_when_bot_edits_bot_controllable_media() {
        test_support::init();
        let media = media::Model { bot_controllable: true, ..fixtures::series_media_model(1, 1) };
        let db = test_support::db::mock()
            .append_query_results([[media.clone()]])
            .append_query_results([[media]])
            .append_query_results([[fixtures::series_model(1)]])
            .append_query_results([[fixtures::title_model(1, 1, "Old Title", true)]])
            .append_query_results([[fixtures::title_model(1, 1, "New Title", true)]])
            .into_connection();

        let result = update(1, series_metadata(1), &fixtures::bot_user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from("New overview"), Value::from(true)]);
    }

    #[tokio::test]
    async fn update_creates_primary_title_when_media_has_none() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(1, 1)]])
            .append_query_results([[fixtures::series_media_model(1, 1)]])
            .append_query_results([[fixtures::series_model(1)]])
            .append_query_results([Vec::<titles::Model>::new()])
            .append_query_results([[fixtures::title_model(5, 1, "New Title", true)]])
            .into_connection();

        let result = update(1, series_metadata(1), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(1), Value::from("New Title"), Value::from(true)]);
    }

    #[tokio::test]
    async fn update_deletes_primary_title_when_metadata_has_no_title() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(1, 1)]])
            .append_query_results([[fixtures::series_media_model(1, 1)]])
            .append_query_results([[fixtures::series_model(1)]])
            .append_query_results([[fixtures::title_model(5, 1, "Old Title", true)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = update(1, SeriesMetadata { title: None, ..series_metadata(1) }, &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "titles""#, &[Value::from(5)]);
    }

    #[tokio::test]
    async fn update_returns_bad_request_when_ids_differ() {
        test_support::init();
        let db = test_support::db::mock().into_connection();

        let result = update(1, series_metadata(2), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(_))));
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn update_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        // The query filters on the user, so the other user's series is not returned.
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = update(5, series_metadata(5), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(5), Value::from(1)]);
    }

    #[tokio::test]
    async fn update_returns_rule_violation_when_language_unknown() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(1, 1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .into_connection();
        let metadata = SeriesMetadata { original_language: Some("xx".into()), ..series_metadata(1) };

        let result = update(1, metadata, &fixtures::user(), &db).await;

        test_support::assert_single_violation(result, |v| matches!(v, RuleViolation::MediaInvalidOriginalLanguage));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn lock_sets_series_lock() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(5, 1)]])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[series_locks::Model { status: true, ..fixtures::series_locks_model(5) }]])
            .into_connection();

        let result = lock(5, "status".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "series_locks""#, &[Value::from(5), Value::from(true)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "media_locks""#);
    }

    #[tokio::test]
    async fn lock_updates_existing_row_when_series_already_has_locks() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(5, 1)]])
            .append_query_results([[series_locks::Model { r#type: true, ..fixtures::series_locks_model(5) }]])
            .append_query_results([[series_locks::Model { r#type: true, status: true, ..fixtures::series_locks_model(5) }]])
            .into_connection();

        let result = lock(5, "status".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "series_locks""#, &[Value::from(true)]);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    #[tokio::test]
    async fn lock_sets_media_lock_when_property_belongs_to_media() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(5, 1)]])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[entities::media_locks::Model { overview: true, ..fixtures::media_locks_model(5) }]])
            .into_connection();

        let result = lock(5, "overview".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "media_locks""#, &[Value::from(5), Value::from(true)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "series_locks""#);
    }

    #[tokio::test]
    async fn lock_returns_bad_request_when_property_unknown() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(5, 1)]])
            .into_connection();

        let result = lock(5, "colour".to_string(), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(_))));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn lock_returns_bad_request_when_property_is_a_movie_lock() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(5, 1)]])
            .into_connection();

        let result = lock(5, "runtime".to_string(), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(_))));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn lock_returns_not_found_when_media_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = lock(5, "status".to_string(), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn unlock_clears_series_lock() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::series_media_model(5, 1)]])
            .append_query_results([[series_locks::Model { status: true, ..fixtures::series_locks_model(5) }]])
            .append_query_results([[fixtures::series_locks_model(5)]])
            .into_connection();

        let result = unlock(5, "status".to_string(), &fixtures::user(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "series_locks""#, &[Value::from(false), Value::from(5)]);
    }

    /// Runs `series::apply_changes` over the window 2026-03-01 to 2026-03-04 with sequential randomness.
    async fn run_apply_changes(media: &media::Model, changes: &PropertyChanges, db: &DbConn, tmdb: &MockTmdb, store: &InMemoryImageStore, jobs: &RecordingJobs) -> Result<(), SrvErr> {
        apply_changes(media, changes, date(2026, 3, 1), date(2026, 3, 4), db, tmdb, store, jobs, &SeqRandom::new()).await
    }

    /// Like `run_apply_changes`, with a TMDB, store and jobs that must not be used.
    async fn run_apply_changes_unused_deps(media: &media::Model, changes: &PropertyChanges, db: &DbConn) -> Result<(), SrvErr> {
        run_apply_changes(media, changes, db, &MockTmdb::new(), &InMemoryImageStore::new(), &RecordingJobs::new()).await
    }

    fn series_changes() -> PropertyChanges {
        property_changes(&[
            ("status", serde_json::json!("Returning Series")),
            ("type", serde_json::json!("Miniseries")),
            ("first_air_date", serde_json::json!("2020-02-03")),
        ])
    }

    /// Series 10 (TMDB id 70) owned by user 1.
    fn tmdb_series() -> media::Model {
        media::Model { tmdb_id: Some(70), ..fixtures::series_media_model(10, 1) }
    }

    #[tokio::test]
    async fn apply_changes_updates_unlocked_properties() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::series_model(10)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_series(), &series_changes(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "series""#, &[
            Value::from("Returning Series"), Value::from("Miniseries"), Value::from(date(2020, 2, 3)), Value::from(10),
        ]);
        assert_no_statement_starting_with(&log, r#"UPDATE "media""#);
    }

    #[tokio::test]
    async fn apply_changes_skips_locked_properties() {
        test_support::init();
        let locks = series_locks::Model { first_air_date: true, status: true, r#type: true, ..fixtures::series_locks_model(10) };
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[locks]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_series(), &series_changes(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_writes_only_unlocked_properties_when_some_locked() {
        test_support::init();
        let locks = series_locks::Model { status: true, ..fixtures::series_locks_model(10) };
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[locks]])
            .append_query_results([[fixtures::series_model(10)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_series(), &series_changes(), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "series""#, &[Value::from("Miniseries"), Value::from(date(2020, 2, 3))]);
        let update = log.iter().find(|s| s.sql.starts_with(r#"UPDATE "series""#)).expect("series update");
        let values = update.values.as_ref().expect("update values");
        assert!(!values.0.contains(&Value::from("Returning Series")), "the locked status must not be written");
    }

    #[tokio::test]
    async fn apply_changes_skips_first_air_date_when_date_unparseable() {
        test_support::init();
        let changes = property_changes(&[("first_air_date", serde_json::json!("sometime in 2020"))]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_series(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_applies_media_properties_when_changes_include_them() {
        test_support::init();
        let changes = property_changes(&[
            ("homepage", serde_json::json!("https://moved.example.test")),
            ("status", serde_json::json!("Canceled")),
        ]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::series_model(10)]])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_series(), &changes, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from("https://moved.example.test")]);
        assert_wrote(&log, r#"UPDATE "series""#, &[Value::from("Canceled")]);
    }

    /// A `season` change for the season with TMDB id `season_id` and number `season_number`, whose
    /// last item has `action`.
    fn season_change(action: &str, season_id: i32, season_number: i32) -> PropertyChanges {
        with_action(property_changes(&[
            ("season", serde_json::json!({"season_id": season_id, "season_number": season_number})),
        ]), action)
    }

    /// An `episode` change for the episode with TMDB id `episode_id`, whose last item has `action`.
    fn episode_change(action: &str, episode_id: i32) -> PropertyChanges {
        with_action(property_changes(&[("episode", serde_json::json!({"episode_id": episode_id}))]), action)
    }

    fn with_action(mut changes: PropertyChanges, action: &str) -> PropertyChanges {
        changes.changes[0].items[0].action = action.into();
        changes
    }

    #[tokio::test]
    async fn apply_changes_upserts_new_season_when_season_added() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_details().withf(|series_id, n| *series_id == 70 && *n == 1).times(1)
            .returning(|series_id, n| Ok(SeasonDetails {
                episodes: vec![tmdb_episode(n, 1), tmdb_episode(n, 2)],
                ..season_details(series_id, n)
            }));
        tmdb.expect_image().times(3).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            // the season is not stored yet, then looked up again by the upsert
            .append_query_results([Vec::<seasons::Model>::new()])
            .append_query_results([Vec::<seasons::Model>::new()]);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster)
            .append_query_results([[fixtures::season_model(31, 10, 1)]]);
        let db = with_saved_tmdb_image(db, 22, ImageType::Still)
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]]);
        let db = with_saved_tmdb_image(db, 23, ImageType::Still)
            .append_query_results([[fixtures::episode_model(42, 31, 1002, 2)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_apply_changes(&tmdb_series(), &season_change("added", 7001, 1), &db, &tmdb, &store, &jobs).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "seasons""#, &[
            Value::from(10), Value::from("Season 1"), Value::from(21), Value::from(7001),
        ]);
        assert_wrote(&log, r#"INSERT INTO "episodes""#, &[Value::from(31), Value::from(1001), Value::from(22)]);
        assert_wrote(&log, r#"INSERT INTO "episodes""#, &[Value::from(31), Value::from(1002), Value::from(23)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "series""#);
        assert_eq!(store.files().len(), 3);
        assert_eq!(sizes(&jobs.take()), vec![(21, 100, 150), (22, 100, 150), (23, 100, 150)]);
    }

    #[tokio::test]
    async fn apply_changes_upserts_season_when_season_missing_locally() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_details().times(1).returning(|series_id, n| Ok(SeasonDetails {
            poster_path: None, ..season_details(series_id, n)
        }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([Vec::<seasons::Model>::new()])
            .append_query_results([Vec::<seasons::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .into_connection();

        // Not reported as added, but there is no such season stored, so it is created.
        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "seasons""#, &[Value::from(10), Value::from("Season 1"), Value::from(7001)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "episodes""#);
    }

    #[tokio::test]
    async fn apply_changes_updates_existing_season_and_adds_poster_when_season_added_again() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_details().times(1).returning(|series_id, n| Ok(SeasonDetails {
            name: Some("Renamed season".into()),
            episodes: vec![TmdbEpisode { still_path: None, ..tmdb_episode(n, 1) }],
            ..season_details(series_id, n)
        }));
        tmdb.expect_image().withf(|path| path == "/series-70-season-1.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[fixtures::season_model(31, 10, 1)]]);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster)
            .append_query_results([[seasons::Model { name: Some("Renamed season".into()), ..fixtures::season_model(31, 10, 1) }]])
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]])
            .into_connection();

        let result = run_apply_changes(&tmdb_series(), &season_change("added", 7001, 1), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "seasons""#, &[Value::from("Renamed season"), Value::from(21), Value::from(31)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "seasons""#);
        assert_wrote(&log, r#"INSERT INTO "episodes""#, &[Value::from(31), Value::from(1001)]);
    }

    #[tokio::test]
    async fn apply_changes_deletes_season_when_season_deleted() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_exec_results([deleted()])
            .into_connection();

        let result = run_apply_changes_unused_deps(&tmdb_series(), &season_change("deleted", 7001, 2), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "seasons""#, &[Value::from(10), Value::from(2)]);
        assert_statement_count(&log, 3);
    }

    #[tokio::test]
    async fn apply_changes_applies_season_property_changes() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().withf(|season_id, start, end| {
            *season_id == 7001 && *start == date(2026, 3, 1) && *end == date(2026, 3, 4)
        }).times(1).returning(|_, _, _| Ok(property_changes(&[
            ("air_date", serde_json::json!("2024-09-01")),
            ("name", serde_json::json!("Renamed season")),
            ("overview", serde_json::json!("New season overview")),
            ("poster_path", serde_json::json!("/new-season-poster.jpg")),
            ("episode_count", serde_json::json!(12)),
        ])));
        tmdb.expect_image().withf(|path| path == "/new-season-poster.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]]);
        let db = with_saved_tmdb_image(db, 21, ImageType::Poster)
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &store, &jobs).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "seasons""#, &[
            Value::from(date(2024, 9, 1)), Value::from("Renamed season"), Value::from("New season overview"),
            Value::from(21), Value::from(31),
        ]);
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/new-season-poster.jpg")]);
        assert_eq!(store.files().len(), 1);
        assert_eq!(sizes(&jobs.take()), vec![(21, 100, 150)]);
    }

    #[tokio::test]
    async fn apply_changes_keeps_season_poster_when_season_already_has_one() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().times(1).returning(|_, _, _| Ok(property_changes(&[
            ("name", serde_json::json!("Renamed season")),
            ("poster_path", serde_json::json!("/new-season-poster.jpg")),
        ])));
        let season = seasons::Model { poster_image_id: Some(8), ..fixtures::season_model(31, 10, 1) };
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[season.clone()]])
            .append_query_results([[season]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &store, &jobs).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "seasons""#, &[Value::from("Renamed season"), Value::from(31)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "images""#);
        assert!(store.files().is_empty());
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn apply_changes_continues_when_season_property_changes_fail() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().times(1).returning(|_, _, _| Err(test_support::tmdb::error("TMDB unavailable")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .into_connection();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn apply_changes_applies_episode_changes() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().times(1).returning(|_, _, _| Ok(episode_change("updated", 1001)));
        tmdb.expect_episode_property_changes().withf(|episode_id, start, end| {
            *episode_id == 1001 && *start == date(2026, 3, 1) && *end == date(2026, 3, 4)
        }).times(1).returning(|_, _, _| Ok(property_changes(&[
            ("name", serde_json::json!("Renamed episode")),
            ("overview", serde_json::json!("New episode overview")),
            ("air_date", serde_json::json!("2024-09-02")),
            ("runtime", serde_json::json!(52)),
            ("still_path", serde_json::json!("/new-still.jpg")),
            ("production_code", serde_json::json!("101")),
        ])));
        tmdb.expect_image().withf(|path| path == "/new-still.jpg").times(1).returning(|_| Ok(png_bytes(300, 450)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]]);
        let db = with_saved_tmdb_image(db, 21, ImageType::Still)
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]])
            .into_connection();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &store, &jobs).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "episodes""#, &[
            Value::from("Renamed episode"), Value::from("New episode overview"), Value::from(date(2024, 9, 2)),
            Value::from(52), Value::from(21), Value::from(41),
        ]);
        assert_wrote(&log, r#"INSERT INTO "images""#, &[Value::from("/new-still.jpg")]);
        // The season itself had nothing to change.
        assert_no_statement_starting_with(&log, r#"UPDATE "seasons""#);
        assert_eq!(store.files().len(), 1);
        assert_eq!(sizes(&jobs.take()), vec![(21, 100, 150)]);
    }

    #[tokio::test]
    async fn apply_changes_keeps_episode_still_when_episode_already_has_one() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().times(1).returning(|_, _, _| Ok(episode_change("updated", 1001)));
        tmdb.expect_episode_property_changes().times(1).returning(|_, _, _| Ok(property_changes(&[
            ("runtime", serde_json::json!(52)),
            ("still_path", serde_json::json!("/new-still.jpg")),
        ])));
        let episode = episodes::Model { still_image_id: Some(8), ..fixtures::episode_model(41, 31, 1001, 1) };
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[episode.clone()]])
            .append_query_results([[episode]])
            .into_connection();
        let jobs = RecordingJobs::new();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &InMemoryImageStore::new(), &jobs).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "episodes""#, &[Value::from(52), Value::from(41)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "images""#);
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn apply_changes_deletes_episode_when_episode_deleted() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().times(1).returning(|_, _, _| Ok(episode_change("deleted", 1001)));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "episodes""#, &[Value::from(31), Value::from(1001)]);
        assert_no_statement_starting_with(&log, r#"DELETE FROM "seasons""#);
    }

    #[tokio::test]
    async fn apply_changes_upserts_season_when_episode_added() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().times(1).returning(|_, _, _| Ok(episode_change("added", 1001)));
        tmdb.expect_season_details().withf(|series_id, n| *series_id == 70 && *n == 1).times(1)
            .returning(|series_id, n| Ok(SeasonDetails {
                poster_path: None,
                episodes: vec![TmdbEpisode { still_path: None, ..tmdb_episode(n, 1) }],
                ..season_details(series_id, n)
            }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            // the stored season, the episode lookup (not stored yet), then the upsert's season lookup
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([Vec::<episodes::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]])
            .into_connection();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "seasons""#, &[Value::from(31)]);
        assert_wrote(&log, r#"INSERT INTO "episodes""#, &[Value::from(31), Value::from(1001)]);
    }

    #[tokio::test]
    async fn apply_changes_continues_when_episode_property_changes_fail() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_season_property_changes().times(1).returning(|_, _, _| Ok(episode_change("updated", 1001)));
        tmdb.expect_episode_property_changes().times(1).returning(|_, _, _| Err(test_support::tmdb::error("TMDB unavailable")));
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<series_locks::Model>::new()])
            .append_query_results([[fixtures::season_model(31, 10, 1)]])
            .append_query_results([[fixtures::episode_model(41, 31, 1001, 1)]])
            .into_connection();

        let result = run_apply_changes(&tmdb_series(), &season_change("updated", 7001, 1), &db, &tmdb, &InMemoryImageStore::new(), &RecordingJobs::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 4);
        assert_no_statement_starting_with(&log, "UPDATE");
    }
}
