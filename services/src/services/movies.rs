use std::env;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel as SeaOrmIntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, QueryOrder, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::{genres, media, media_genres, movies, titles};
use entities::prelude::{Genres, Languages, Logs, Media, Movies, Sources, Tags, Titles, Watchlist};
use entities::sea_orm_active_enums::MediaType;
use integrations::tmdb;
use views::genres::Genre;
use views::languages::Language;
use views::logs::Log;
use views::media::MediaIndex;
use views::movies::{MovieDetails, MovieMetadata};
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;
use views::users::CurrentUser;
use crate::infrastructure::{RuleViolation, SrvErr};
use crate::services;
use crate::infrastructure::traits::IntoActiveModel;

pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(bool, i32), SrvErr> {
    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie)).one(db).await;
    match med_res {
        Ok(media) => {
            if media.is_some() {
                return Ok((false, media.unwrap().id));
            }
        }
        Err(err) => { return Err(SrvErr::DB(err)); }
    };

    let tran = db.begin().await?;

    let tmdb_movie = match tmdb::services::movies::details(tmdb_id).await {
        Ok(movie) => { movie }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let mut media: media::ActiveModel = tmdb_movie.into_active_model();
    let mut movie: movies::ActiveModel = tmdb_movie.into_active_model();

    media.user_id = Set(user.id);
    let inserted_media = media.insert(db).await?;
    movie.id = Set(inserted_media.id);
    movie.insert(db).await?;

    for genre in &tmdb_movie.genres {
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id))
            .filter(genres::Column::Type.eq(MediaType::Movie)).one(db).await?;
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
        title: Set(tmdb_movie.title)
    };
    model.insert(db).await?;

    let titles = tmdb::services::movies::alternative_titles(tmdb_id).await?;
    for title in titles.titles {
        let model = titles::ActiveModel {
            id: NotSet,
            media_id: Set(inserted_media.id),
            primary: Set(false),
            title: Set(title.title)
        };
        model.insert(db).await?;
    }

    tran.commit().await?;
    Ok((true, inserted_media.id))
}


pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media_w_titles = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie)).find_also_related(Titles)
        .filter(titles::Column::Primary.eq(true)).order_by_asc(titles::Column::Title).all(db).await?;
    let indexes = services::media::build_media_indexes(media_w_titles);
    Ok(indexes)
}

pub async fn details(movie_id: i32, user: &CurrentUser, db: &DbConn) -> Result<Option<MovieDetails>, SrvErr> {
    let db_media = Media::find().filter(media::Column::Id.eq(movie_id))
        .filter(media::Column::UserId.eq(user.id)).filter(media::Column::Type.eq(MediaType::Movie)).one(db).await?;

    if db_media.is_none() {
        return Ok(None);
    }
    let db_media = db_media.unwrap();

    let db_movie = db_media.find_related(Movies).one(db).await?;
    let db_movie = db_movie.expect("DB in invalid state!");

    let db_titles = db_media.find_related(Titles).all(db).await?;

    let language = if let Some(language_id) = db_media.original_language.clone() {
        Languages::find_by_id(language_id).one(db).await?.map(|l| {
            Language::from(l)
        })
    } else {
        None
    };

    let genres = db_media.find_related(Genres).all(db).await?.iter().map(|m| {
        Genre::from(m)
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
        env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!")
    };

    let on_watchlist = db_media.find_related(Watchlist).count(db).await? != 0;

    let movie = MovieDetails {
        id: movie_id,
        poster_path: db_media.poster_path,
        backdrop_path: db_media.backdrop_path,
        stars: db_media.stars,
        title,
        alternative_titles,
        release_date: db_movie.release_date,
        runtime: db_movie.runtime,
        status: db_movie.status,
        overview: db_media.overview,
        tmdb_vote_average: db_media.tmdb_vote_average,
        on_watchlist,
        original_language: language,
        genres,
        tags,
        sources,
        logs,
    };

    Ok(Some(movie))
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
        backdrop_path: db_media.backdrop_path,
        homepage: db_media.homepage,
        tmdb_id: db_media.tmdb_id,
        imdb_id: db_media.imdb_id,
        title,
        overview: db_media.overview,
        poster_path: db_media.poster_path,
        tmdb_vote_average: db_media.tmdb_vote_average,
        original_language: db_media.original_language,
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

    if metadata.original_language != media.original_language && metadata.original_language.is_some() {
        let found = Languages::find_by_id(metadata.original_language.as_ref().unwrap()).count(db).await?;
        if found == 0 {
            rule_violations.push(RuleViolation::MediaInvalidOriginalLanguage);
        }
    }

    if let Some(avg) = metadata.tmdb_vote_average {
        if avg > 10.0 || avg < 0.0 {
            rule_violations.push(RuleViolation::MediaTmdbVoteAverageOutOfBounds);
        }
    }

    if let Some(runtime) = metadata.runtime {
        if runtime < 0 {
            rule_violations.push(RuleViolation::MovieNegativeRuntime);
        }
    }

    if !rule_violations.is_empty() {
        return Err(SrvErr::RuleViolation(rule_violations));
    }

    let tran = db.begin().await?;

    let media_am: media::ActiveModel = (&metadata).into_active_model();
    media_am.update(db).await?;
    let movie_am: movies::ActiveModel = (&metadata).into_active_model();
    movie_am.update(db).await?;

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