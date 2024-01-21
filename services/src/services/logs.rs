use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, QueryFilter, TransactionTrait};
use entities::{logs, media, sea_orm_active_enums, sources};
use entities::prelude::{Logs, Sources};
use views::logs::{LogCreate, LogUpdate};
use views::media::MediaType;
use views::users::CurrentUser;
use crate::infrastructure::{RuleViolation, SrvErr};
use crate::infrastructure::traits::IntoActiveModel;

pub async fn create(media_id: i32, log: &LogCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let mut rule_violations = Vec::new();
    if let Some(rating) = log.rating {
        if rating < 0f32 || rating > 10f32 {
            rule_violations.push(RuleViolation::LogRatingOutOfBounds);
        }
    }

    if !rule_violations.is_empty() {
        return Err(SrvErr::RuleViolation(rule_violations));
    }

    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let source = media.find_related(Sources).filter(sources::Column::Name.eq(&log.source)).one(db).await?;
    if source.is_none() {
        return Err(SrvErr::NotFound);
    }
    let source = source.unwrap();

    let mut model = log.into_active_model();
    model.media_id = Set(media_id);
    model.source_id = Set(source.id);
    model.insert(db).await?;
    Ok(())
}

pub async fn update(media_id: i32, log_id: i32, log: &LogUpdate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if log.id != log_id {
        return Err(SrvErr::BadRequest("Endpoint log id does not match log id in metadata!".to_string()));
    }

    let mut rule_violations = Vec::new();
    if let Some(rating) = log .rating {
        if rating < 0f32 || rating > 10f32 {
            rule_violations.push(RuleViolation::LogRatingOutOfBounds);
        }
    }

    if !rule_violations.is_empty() {
        return Err(SrvErr::RuleViolation(rule_violations));
    }

    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let model = media.find_related(Logs).filter(logs::Column::Id.eq(log_id)).one(db).await?;
    if model.is_none() {
        return Err(SrvErr::NotFound);
    }

    let source = media.find_related(Sources).filter(sources::Column::Name.eq(&log.source)).one(db).await?;
    if source.is_none() {
        return Err(SrvErr::NotFound);
    }
    let source = source.unwrap();

    let mut model: logs::ActiveModel = model.unwrap().into();
    model.source_id = Set(source.id);
    model.date = Set(log.date);
    model.completed = Set(log.completed);
    model.rating = Set(log.rating);
    model.comment = Set(log.comment.clone());

    let tran = db.begin().await?;

    model.update(db).await?;

    tran.commit().await?;

    Ok(())
}

pub async fn delete(media_id: i32, log_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let log = media.find_related(Logs).filter(logs::Column::Id.eq(log_id)).one(db).await?;
    if log.is_none() {
        return Err(SrvErr::NotFound);
    }
    let log = log.unwrap();

    let tran = db.begin().await?;

    log.delete(db).await?;

    tran.commit().await?;

    Ok(())
}