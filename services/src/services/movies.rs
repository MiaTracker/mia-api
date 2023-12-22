use std::env;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, NotSet, QueryFilter, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::{genres, media, media_genres, movies, sea_orm_active_enums, titles, user_media};
use entities::prelude::{Genres, Languages, Logs, Media, Movies, Sources, Tags, Titles, UserMedia};
use entities::sea_orm_active_enums::MediaType;
use integrations::tmdb;
use views::genres::Genre;
use views::infrastructure::traits::IntoActiveModel;
use views::languages::Language;
use views::logs::Log;
use views::media::MediaIndex;
use views::movies::MovieDetails;
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(tmdb_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let trans = db.begin().await?;

    let tmdb_movie = match tmdb::movies::details(tmdb_id).await {
        Ok(movie) => { movie }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id))
        .filter(media::Column::Type.eq(sea_orm_active_enums::MediaType::Movie)).one(db).await;
    let med_opt = match med_res {
        Ok(media) => { media }
        Err(err) => { return Err(SrvErr::DB(err)); }
    };
    let mut media = <&views::tmdb::MovieDetails as IntoActiveModel<media::ActiveModel>>::into_active_model(&tmdb_movie);
    let mut movie = <&views::tmdb::MovieDetails as IntoActiveModel<movies::ActiveModel>>::into_active_model(&tmdb_movie);
    let inserted_media;
    if let Some(med) = med_opt {
        media.id = Set(med.id);
        media.date_added = NotSet;
        inserted_media = media.update(db).await?;
        movie.id = Set(med.id);
        movie.update(db).await?;
    } else {
        inserted_media = media.insert(db).await?;
        movie.id = Set(inserted_media.id);
        movie.insert(db).await?;
    }

    let user_media = user_media::Entity::find()
        .filter(user_media::Column::UserId.eq(user.id))
        .filter(user_media::Column::MediaId.eq(inserted_media.id))
        .one(db).await?;
    if user_media.is_none() {
        let user_media_am = user_media::ActiveModel {
            media_id: Set(inserted_media.id),
            user_id: Set(user.id),
            stars: Set(None),
        };
        user_media_am.insert(db).await?;
    }

    for genre in &tmdb_movie.genres {
        let mut model = <&views::tmdb::Genre as IntoActiveModel<genres::ActiveModel>>::into_active_model(&genre);
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id)).one(db).await?;
        let inserted;
        if let Some(existing_genre) = existing {
            model.id = Set(existing_genre.id);
            inserted = model.update(db).await?;
        } else {
            inserted = model.insert(db).await?;
        }

        let media_genre = media_genres::Entity::find()
            .filter(media_genres::Column::MediaId.eq(inserted_media.id))
            .filter(media_genres::Column::GenreId.eq(inserted.id))
            .one(db).await?;
        if media_genre.is_none() {
            let media_genre = media_genres::ActiveModel {
                media_id: Set(inserted_media.id),
                genre_id: Set(inserted.id),
            };
            media_genre.insert(db).await?;
        }
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

    trans.commit().await?;
    Ok(())
}


pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let user_media = UserMedia::find().filter(user_media::Column::UserId.eq(user.id)).all(db).await?;
    let mut indexes = Vec::with_capacity(user_media.len());
    for um in user_media {
        let media = um.find_related(Media).one(db).await?;
        if media.is_none() {
            return Err(SrvErr::Internal("User media exists without a media reference!".to_string()));
        }
        let media = media.unwrap();
        if media.r#type != MediaType::Movie {
            continue;
        }

        let title = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
        let title = if let Some(title) = title {
            title.title
        } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

        let index = MediaIndex {
            id: media.id,
            r#type: views::media::MediaType::from(media.r#type),
            poster_path: media.poster_path,
            stars: um.stars,
            title,
        };
        indexes.push(index);
    }

    Ok(indexes)
}

pub async fn details(user: &CurrentUser, movie_id: i32, db: &DbConn) -> Result<Option<MovieDetails>, SrvErr> {
    let db_user_media = UserMedia::find().filter(user_media::Column::UserId.eq(user.id)).filter(user_media::Column::MediaId.eq(movie_id)).one(db).await?;
    if db_user_media.is_none() {
        return Ok(None);
    }
    let db_user_media = db_user_media.unwrap();

    let db_media = Media::find_by_id(movie_id).one(db).await?;
    let db_media = db_media.expect("DB in invalid state!");

    if db_media.r#type != MediaType::Movie { return Ok(None); }

    let db_movie = Movies::find_by_id(movie_id).one(db).await?;
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
        stars: db_user_media.stars,
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