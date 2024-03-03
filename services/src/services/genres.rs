use cruet::Inflector;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, QuerySelect, Set, TransactionTrait};
use entities::{genres, media, media_genres, sea_orm_active_enums};
use entities::prelude::{Genres, Media, MediaGenres};
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

    let existing_db = genres::Entity::find().filter(genres::Column::Name.eq(&name))
        .filter(genres::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into())).one(db).await?;

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

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    tran.commit().await?;
    Ok(true)
}

pub async fn index(media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<Vec<String>, SrvErr> {
    let mut select = Genres::find().distinct().inner_join(Media).filter(media::Column::UserId.eq(user.id));
    if let Some(media_type) = media_type {
        select = select.filter(genres::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
            .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()));
    }
    let genres = select.all(db).await?.iter().map(|m| {
        m.name.clone()
    }).collect();

    Ok(genres)
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

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    tran.commit().await?;

    Ok(())
}