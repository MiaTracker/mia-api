use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, NotSet, QueryFilter, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::{genres, media, media_genres, movies, user_media};
use integrations::tmdb;
use views::infrastructure::traits::IntoActiveModel;
use views::media::MediaType;
use views::tmdb::{MovieDetails, Genre};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(tmdb_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let trans = db.begin().await?;
    match media_type {
        MediaType::Movie => { create_movie(tmdb_id, user, db).await?; }
        MediaType::Series => { create_series(tmdb_id, user, db).await? }
    }

    trans.commit().await?;
    Ok(())
}

async fn create_movie(tmdb_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let tmdb_movie = match tmdb::movies::details(tmdb_id).await {
        Ok(movie) => { movie }
        Err(err) => { return Err(SrvErr::from(err)); }
    };

    let med_res = media::Entity::find().filter(media::Column::TmdbId.eq(tmdb_id)).one(db).await;
    let med_opt = match med_res {
        Ok(media) => { media }
        Err(err) => { return Err(SrvErr::DB(err)); }
    };
    let mut media = <&MovieDetails as IntoActiveModel<media::ActiveModel>>::into_active_model(&tmdb_movie);
    let mut movie = <&MovieDetails as IntoActiveModel<movies::ActiveModel>>::into_active_model(&tmdb_movie);
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
        let mut model = <&Genre as IntoActiveModel<genres::ActiveModel>>::into_active_model(&genre);
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

    Ok(())
}

async fn create_series(_tmdb_id: i32, _user: &CurrentUser, _db: &DbConn) {
    panic!("Not implemented!");
}