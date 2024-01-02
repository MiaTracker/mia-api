use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, NotSet, PaginatorTrait, QueryFilter, Set, TransactionTrait};
use entities::{media, sea_orm_active_enums, titles};
use entities::prelude::Titles;
use views::media::MediaType;
use views::titles::TitleCreate;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, title: &TitleCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let existing = media.find_related(Titles).filter(titles::Column::Title.eq(&title.name)).one(db).await?;
    if existing.is_some() {
        return Ok(false);
    }

    let primary = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).count(db).await? == 0;
    let model = titles::ActiveModel {
        id: NotSet,
        media_id: Set(media_id),
        primary: Set(primary),
        title: Set(title.name.clone()),
    };

    let trans = db.begin().await?;

    model.insert(db).await?;

    trans.commit().await?;

    Ok(true)
}

pub async fn set_primary(media_id: i32, title_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let title = media.find_related(Titles).filter(titles::Column::Id.eq(title_id)).one(db).await?;
    if title.is_none() {
        return Err(SrvErr::NotFound);
    }

    let trans = db.begin().await?;

    let primary = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
    if primary.is_some() {
        let mut model: titles::ActiveModel = primary.unwrap().into();
        model.primary = Set(false);
        model.update(db).await?;
    }

    let mut title: titles::ActiveModel = title.unwrap().into();
    title.primary = Set(true);
    title.update(db).await?;

    trans.commit().await?;

    Ok(())
}

pub async fn delete(media_id: i32, title_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id)
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let title = media.find_related(Titles).filter(titles::Column::Id.eq(title_id)).one(db).await?;
    if title.is_none() {
        return Err(SrvErr::NotFound);
    }
    let title = title.unwrap();

    let trans = db.begin().await?;

    if title.primary {
        let first = media.find_related(Titles).filter(titles::Column::Primary.eq(false)).one(db).await?;
        title.delete(db).await?;
        if first.is_some() {
            let mut model: titles::ActiveModel = first.unwrap().into();
            model.primary = Set(true);
            model.update(db).await?;
        }
    } else {
        title.delete(db).await?;
    }

    trans.commit().await?;

    Ok(())
}