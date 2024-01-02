use inflector::Inflector;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, NotSet, PaginatorTrait, QueryFilter, Set, TransactionTrait};
use entities::{genres, media, media_genres, sea_orm_active_enums};
use entities::prelude::{Genres, MediaGenres};
use views::genres::GenreCreate;
use views::media::MediaType;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, genre: &GenreCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let name = genre.name.to_title_case();

    let existing = media.find_related(Genres).filter(genres::Column::Name.eq(&name)).one(db).await?;
    if existing.is_some() {
        return Ok(false);
    }

    let existing_db = genres::Entity::find().filter(genres::Column::Name.eq(&name)).one(db).await?;

    let tran = db.begin().await?;

    if let Some(existing) = existing_db {
        let rel = media_genres::ActiveModel {
            media_id: Set(media_id),
            genre_id: Set(existing.id),
        };
        rel.insert(db).await?;
    } else {
        let model = genres::ActiveModel {
            id: NotSet,
            tmdb_id: NotSet,
            name: Set(name),
            r#type: Set(media_type.into()),
        };
        let model = model.insert(db).await?;
        let rel = media_genres::ActiveModel {
            media_id: Set(media_id),
            genre_id: Set(model.id)
        };
        rel.insert(db).await?;
    }

    tran.commit().await?;
    Ok(true)
}

pub async fn delete(media_id: i32, genre_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id)
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let media_genre = media.find_related(MediaGenres).filter(media_genres::Column::GenreId.eq(genre_id)).one(db).await?;
    if media_genre.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media_genre = media_genre.unwrap();

    let tran = db.begin().await?;

    let genre = media_genre.find_related(Genres).one(db).await?.expect("DB in invalid state!");
    media_genre.delete(db).await?;
    if genre.tmdb_id.is_none() {
        let count = genre.find_related(MediaGenres).count(db).await?;
        if count == 0 {
            genre.delete(db).await?;
        }
    }

    tran.commit().await?;

    Ok(())
}