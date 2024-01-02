use inflector::Inflector;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, NotSet, PaginatorTrait, QueryFilter, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::prelude::{MediaTags, Tags};
use entities::{media, media_tags, sea_orm_active_enums, tags};
use views::media::MediaType;
use views::tags::TagCreate;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, tag: &TagCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let name = tag.name.to_title_case();

    let existing = media.find_related(Tags).filter(tags::Column::Name.eq(&name)).one(db).await?;
    if existing.is_some() {
        return Ok(false);
    }

    let existing_db = tags::Entity::find().filter(tags::Column::Name.eq(&name)).one(db).await?;

    let trans = db.begin().await?;

    if let Some(existing) = existing_db {
        let rel = media_tags::ActiveModel {
            media_id: Set(media_id),
            tag_id: Set(existing.id),
        };
        rel.insert(db).await?;
    } else {
        let model = tags::ActiveModel {
            id: NotSet,
            name: Set(name),
        };
        let model = model.insert(db).await?;
        let rel = media_tags::ActiveModel {
            media_id: Set(media_id),
            tag_id: Set(model.id),
        };
        rel.insert(db).await?;
    }

    trans.commit().await?;
    Ok(true)
}

pub async fn delete(media_id: i32, tag_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id)
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let media_tag = media.find_related(MediaTags).filter(media_tags::Column::TagId.eq(tag_id)).one(db).await?;
    if let Some(media_tag) = media_tag {
        let trans = db.begin().await?;

        let tag = media_tag.find_related(Tags).one(db).await?.expect("DB in invalid state!");
        media_tag.delete(db).await?;
        let count = tag.find_related(MediaTags).count(db).await?;
        if count == 0 {
            tag.delete(db).await?;
        }

        trans.commit().await?;
    }
    Ok(())
}