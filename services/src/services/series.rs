use entities::prelude::{Episodes, ImageSizes, MediaLocks, Seasons, SeriesLocks};
use entities::prelude::Watchlist;
use entities::prelude::Logs;
use entities::prelude::Sources;
use entities::prelude::Titles;
use entities::prelude::Languages;
use entities::prelude::Media;
use entities::prelude::Series;
use entities::prelude::Genres;
use entities::prelude::Tags;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, DbConn, ColumnTrait, EntityTrait, ModelTrait, NotSet, QueryFilter, TransactionTrait, PaginatorTrait, IntoActiveModel as SeaOrmIntoActiveModel, QueryOrder, QuerySelect};
use sea_orm::sea_query::OnConflict;
use entities::{episodes, functions, genres, image_sizes, media, media_genres, seasons, series, series_locks, titles};
use integrations::tmdb::views::{SeasonDetails, SeriesTitle, TmdbEpisode};
use views::users::CurrentUser;
use crate::infrastructure::{constants, RuleViolation, SrvErr};
use entities::sea_orm_active_enums::{ImageType, MediaType};
use entities::traits::linked::MediaPosters;
use chrono::NaiveDate;
use integrations::tmdb;
use integrations::tmdb::views::PropertyChanges;
use views::languages::Language;
use views::logs::Log;
use views::media::{MediaIndex, PageReq};
use views::series::SeriesMetadata;
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;
use entities::traits::locks::{SetLock, ToLocks};
use infrastructure::config;
use log::error;
use crate::infrastructure::traits::IntoActiveModel;
use crate::media::{fetch_media, try_set_media_lock};
use crate::services;
use crate::services::images::{fetch_backdrop_and_poster, save_tmdb_image};

pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(bool, i32), SrvErr> {
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

    let tran = db.begin().await?;

    let tmdb_series = match tmdb::services::series::details(tmdb_id).await {
        Ok(series) => { series }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let mut media: media::ActiveModel = tmdb_series.into_active_model();
    let mut series: series::ActiveModel = tmdb_series.into_active_model();

    media.backdrop_image_id = Set(
        match tmdb_series.backdrop_path {
            None =>  None,
            Some(path) => {
                Some(save_tmdb_image(path.as_str(), ImageType::Backdrop, db).await?)
            }
        }
    );
    media.poster_image_id = Set(
        match tmdb_series.poster_path {
            None =>  None,
            Some(path) => {
                Some(save_tmdb_image(path.as_str(), ImageType::Poster, db).await?)
            }
        }
    );

    media.user_id = Set(user.id);
    media.bot_controllable = Set(user.though_bot);
    let inserted_media = media.insert(db).await?;
    series.id = Set(inserted_media.id);
    series.insert(db).await?;

    if let Some(seasons) = &tmdb_series.seasons {
        for season in seasons {
            if let Some(sn) = season.season_number {
                upsert_season_with_episodes(tmdb_id, inserted_media.id, sn, db).await?;
            }
        }
    }

    for genre in &tmdb_series.genres {
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id))
            .filter(genres::Column::Type.eq(MediaType::Series)).one(db).await?;
        if existing.is_none() {
            return Err(SrvErr::MasterdataOutdated);
        }

        let media_genre = media_genres::ActiveModel {
            media_id: Set(inserted_media.id),
            genre_id: Set(existing.unwrap().id),
        };
        media_genre.insert(db).await?;
    }


    let model = titles::ActiveModel {
        id: NotSet,
        media_id: Set(inserted_media.id),
        primary: Set(true),
        title: Set(tmdb_series.name)
    };
    model.insert(db).await?;

    let titles = tmdb::services::series::alternative_titles(tmdb_id).await?;
    let titles = titles.results.iter().filter(|x| {
        if x.iso_3166_1 == constants::TMDB_3166_1 { return true }
        if let Some(countries) = &tmdb_series.production_countries {
            return countries.iter().any(|y| { y.iso_3166_1 == x.iso_3166_1 })
        }
        true
    }).collect::<Vec<&SeriesTitle>>();
    for title in titles {
        let model = titles::ActiveModel {
            id: NotSet,
            media_id: Set(inserted_media.id),
            primary: Set(false),
            title: Set(title.title.clone())
        };
        model.insert(db).await?;
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

    let db_series =  db_media.find_related(Series).one(db).await?;
    let db_series = db_series.expect("DB in invalid state!");

    let db_titles = db_media.find_related(Titles).all(db).await?;

    let language = if let Some(language_id) = db_media.original_language.clone() {
        Languages::find_by_id(language_id).one(db).await?.map(|l| {
            Language::from(l)
        })
    } else {
        None
    };

    let genres = db_media.find_related(Genres).all(db).await?.iter().map(|m| {
        views::genres::Genre::from(m)
    }).collect();
    let tags = db_media.find_related(Tags).all(db).await?.iter().map(|m| {
        Tag::from(m)
    }).collect();
    let sources: Vec<Source> = db_media.find_related(Sources).all(db).await?.iter().map(|m| {
        Source::from(m)
    }).collect();
    let logs = db_media.find_related(Logs).all(db).await?.iter().map(|m| {
        let mut log = Log::from(m);
        let source = sources.iter().find(|&x| { x.id == m.source_id });
        if let Some(source) = source {
            log.source = source.name.clone();
        }
        log
    }).collect();

    let mut title = None;
    let alternative_titles = db_titles.iter().filter_map(|t| {
        if t.primary {
            title = Some(t);
            None
        } else {
            Some(AlternativeTitle::from(t))
        }
    }).collect();
    let title = if let Some(t) = title {
        t.title.clone()
    } else {
        config().media.unset_title.clone()
    };

    let on_watchlist = db_media.find_related(Watchlist).count(db).await? != 0;

    let media_locks = db_media.find_related(MediaLocks).one(db).await?;
    let series_locks = db_series.find_related(SeriesLocks).one(db).await?;
    let mut locks = media_locks.to_locks();
    locks.append(&mut series_locks.to_locks());

    let (backdrop, poster) = fetch_backdrop_and_poster(&db_media, db).await?;

    let number_of_seasons = Some(db_series.find_related(Seasons).count(db).await? as i32);
    let number_of_episodes = Some(
        Episodes::find()
            .inner_join(seasons::Entity)
            .filter(seasons::Column::SeriesId.eq(db_series.id))
            .count(db)
            .await? as i32
    );

    let series = views::series::SeriesDetails {
        id: db_media.id,
        poster,
        backdrop,
        stars: db_media.stars,
        title,
        alternative_titles,
        first_air_date: db_series.first_air_date,
        number_of_episodes,
        number_of_seasons,
        status: db_series.status,
        r#type: db_series.r#type,
        overview: db_media.overview,
        tmdb_id: db_media.tmdb_id,
        tmdb_vote_average: db_media.tmdb_vote_average,
        on_watchlist,
        original_language: language,
        genres,
        tags,
        sources,
        logs,
        locks
    };

    Ok(Some(series))
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
    media_am.update(db).await?;
    let series_am: series::ActiveModel = (&metadata).into_active_model();
    series_am.update(db).await?;

    let db_title = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;

    if let Some(title) = metadata.title {
        if let Some(title_mod) = db_title {
            let mut am = title_mod.into_active_model();
            am.title = Set(title);
            am.update(db).await?;
        } else {
            let am = titles::ActiveModel {
                id: NotSet,
                media_id: Set(media.id),
                primary: Set(true),
                title: Set(title),
            };
            am.insert(db).await?;
        }
    } else {
        if let Some(title_mod) = db_title {
            title_mod.delete(db).await?;
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
pub async fn apply_changes(media: &entities::media::Model, changes: &PropertyChanges, start_date: NaiveDate, end_date: NaiveDate, db: &DbConn) -> Result<(), SrvErr> {
    crate::media::apply_changes(media, changes, db).await?;

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
                            if let Err(e) = upsert_season_with_episodes(series_tmdb_id, media.id, sn, db).await {
                                error!("Failed to create new season series_id={} season={}: {}", media.id, sn, e);
                            }
                        }
                    } else if let Some(season) = season_opt {
                        match tmdb::services::series::season_property_changes(stid, start_date, end_date).await {
                            Err(e) => error!("Failed season property_changes series_id={} season={}: {}", media.id, sn, e),
                            Ok(sc) => {
                                apply_season_property_changes(&season, &sc, db).await?;

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
                                                if let Err(e) = upsert_season_with_episodes(series_tmdb_id, media.id, sn, db).await {
                                                    error!("Failed to create new episode series_id={} season={}: {}", media.id, sn, e);
                                                }
                                            }
                                        } else if let Some(ep) = episode {
                                            match tmdb::services::series::episode_property_changes(etid, start_date, end_date).await {
                                                Err(e) => error!("Failed episode property_changes ep_tmdb_id={}: {}", etid, e),
                                                Ok(ec) => {
                                                    if let Err(e) = apply_episode_changes(&ep, &ec, db).await {
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

async fn upsert_season_with_episodes(
    series_tmdb_id: i32,
    series_db_id: i32,
    season_number: i32,
    db: &DbConn,
) -> Result<seasons::Model, SrvErr> {
    let season_details = tmdb::services::series::season_details(series_tmdb_id, season_number).await?;

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
                    am.poster_image_id = Set(Some(save_tmdb_image(path, ImageType::Poster, db).await?));
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
                am.poster_image_id = Set(Some(save_tmdb_image(path, ImageType::Poster, db).await?));
            }
        }
        am.insert(db).await?
    };

    for episode in &season_details.episodes {
        let mut ep_am = <&TmdbEpisode as IntoActiveModel<episodes::ActiveModel>>::into_active_model(episode);
        ep_am.season_id = Set(season_model.id);
        let still_image_id = if let Some(path) = &episode.still_path {
            if !path.is_empty() {
                save_tmdb_image(path, ImageType::Still, db).await.ok()
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

async fn apply_season_property_changes(
    season: &seasons::Model,
    changes: &PropertyChanges,
    db: &DbConn,
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
                        let id = save_tmdb_image(s, ImageType::Poster, db).await?;
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
                        let id = save_tmdb_image(s, ImageType::Still, db).await?;
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