use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, QueryFilter};
use sea_orm::ActiveValue::Set;
use entities::prelude::{MediaTags};
use entities::{media_tags, tags};
use views::infrastructure::traits::IntoActiveModel;
use views::tags::TagCreate;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, tag: &TagCreate, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let existing_db = tags::Entity::find().filter(tags::Column::Name.eq(&tag.name)).one(db).await?;
    if let Some(existing) = existing_db {
        let rel_db = MediaTags::find()
            .filter(media_tags::Column::TagId.eq(existing.id))
            .filter(media_tags::Column::UserId.eq(user.id))
            .one(db).await?;
        if rel_db.is_none() {
            let rel = media_tags::ActiveModel {
                media_id: Set(media_id),
                tag_id: Set(existing.id),
                user_id: Set(user.id),
            };
            rel.insert(db).await?;
        }
    } else {
        let model = tag.into_active_model();
        let model = model.insert(db).await?;
        let rel = media_tags::ActiveModel {
            media_id: Set(media_id),
            tag_id: Set(model.id),
            user_id: Set(user.id),
        };
        rel.insert(db).await?;
    }

    Ok(())
}

pub async fn delete(media_id: i32, tag_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let rel = MediaTags::find().filter(media_tags::Column::MediaId.eq(media_id))
        .filter(media_tags::Column::TagId.eq(tag_id))
        .filter(media_tags::Column::UserId.eq(user.id)).one(db).await?;
    if rel.is_none() { return Ok(()) }
    let rel = rel.unwrap();
    rel.delete(db).await?;
    MediaTags::delete_many().filter(media_tags::Column::TagId.eq(tag_id)).exec(db).await?;

    Ok(())
}