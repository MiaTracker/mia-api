use std::env;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, ModelTrait, QueryFilter};
use entities::{titles, user_media};
use entities::prelude::{Genres, Languages, Logs, Media, Movies, Sources, Tags, Titles, UserMedia};
use entities::sea_orm_active_enums::MediaType;
use views::genres::Genre;
use views::languages::Language;
use views::logs::Log;
use views::media::MediaIndex;
use views::movies::MovieDetails;
use views::sources::Source;
use views::tags::Tag;
use views::titles::AlternativeTitle;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<MediaIndex>, SrvErr> {
    let user_media = UserMedia::find().filter(user_media::Column::UserId.eq(user.id)).all(db).await?;
    let mut indexes = Vec::with_capacity(user_media.len());
    for um in user_media {
        let media = um.find_related(Media).one(db).await?;
        if media.is_none() {
            return Err(SrvErr::Internal("User media exists without a media reference!".to_string()));
        }
        let media = media.unwrap();

        let title = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
        let title = if let Some(title) = title {
            title.title
        } else { env::var("UNSET_MEDIA_TITLE").expect("UNSET_MEDIA_TITLE not set!") };

        let stars;
        if media.r#type == MediaType::Movie {
            let movie = media.find_related(Movies).one(db).await?;
            if let Some(movie) = movie {
                stars = movie.stars;
            } else {
                return Err(SrvErr::Internal("Media of type movie exists without a movie reference!".to_string()));
            }
        } else {
            stars = None;
            //TODO: series
        }


        let index = MediaIndex {
            id: media.id,
            r#type: views::media::MediaType::from(media.r#type),
            poster_path: media.poster_path,
            stars,
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
        stars: db_movie.stars,
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