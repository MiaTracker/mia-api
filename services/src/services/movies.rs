use std::env;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, NotSet, QueryFilter, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::{genres, media, media_genres, movies, titles};
use entities::prelude::{Genres, Languages, Logs, Media, Movies, Sources, Tags, Titles};
use entities::sea_orm_active_enums::MediaType;
use integrations::tmdb;
use views::genres::Genre;
use views::infrastructure::traits::IntoActiveModel;
use views::languages::Language;
use views::logs::Log;
use views::media::MediaIndex;
use views::movies::{MovieDetails, MovieMetadata};
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;
use crate::services;

pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie)).one(db).await;
    match med_res {
        Ok(media) => {
            if media.is_some() {
                return Ok(false);
            }
        }
        Err(err) => { return Err(SrvErr::DB(err)); }
    };

    let tran = db.begin().await?;

    let tmdb_movie = match tmdb::movies::details(tmdb_id).await {
        Ok(movie) => { movie }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let mut media = <&views::tmdb::MovieDetails as IntoActiveModel<media::ActiveModel>>::into_active_model(&tmdb_movie);
    let mut movie = <&views::tmdb::MovieDetails as IntoActiveModel<movies::ActiveModel>>::into_active_model(&tmdb_movie);

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

    let titles = tmdb::movies::alternative_titles(tmdb_id).await?;
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
    Ok(true)
}


pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let media = Media::find().filter(media::Column::UserId.eq(user.id))
        .filter(media::Column::Type.eq(MediaType::Movie)).all(db).await?;
    let mut indexes = Vec::with_capacity(media.len());
    for m in media {
        let title = m.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
        let title = if let Some(title) = title {
            title.title
        } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

        let index = MediaIndex {
            id: m.id,
            r#type: views::media::MediaType::from(m.r#type),
            poster_path: m.poster_path,
            stars: m.stars,
            title,
        };
        indexes.push(index);
    }

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
    let logs = db_media.find_related(Logs).all(db).await?.iter().map(|m| {
        Log::from(m)
    }).collect();
    let sources = db_media.find_related(Sources).all(db).await?.iter().map(|m| {
        Source::from(m)
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

    let metadata = MovieMetadata {
        id: movie.id,
        backdrop_path: db_media.backdrop_path,
        homepage: db_media.homepage,
        tmdb_id: db_media.tmdb_id,
        overview: db_media.overview,
        poster_path: db_media.poster_path,
        tmdb_vote_average: db_media.tmdb_vote_average,
        original_language: db_media.original_language,
        release_date: Some(movie.release_date),
        runtime: movie.runtime,
        status: Some(movie.status),
    };
    Ok(metadata)
}

pub async fn delete(movie_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    services::media::delete(movie_id, user, db).await
}