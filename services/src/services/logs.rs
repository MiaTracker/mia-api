use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, FromQueryResult, IntoActiveModel as SOIntoActiveModel, ModelTrait, QueryFilter, QuerySelect, TransactionTrait};
use sea_orm::prelude::Expr;
use entities::{logs, media, sea_orm_active_enums, sources};
use entities::prelude::{Logs, Media, Sources};
use views::logs::{Log, LogCreate, LogUpdate};
use views::media::MediaType;
use views::users::CurrentUser;
use crate::infrastructure::{RuleViolation, SrvErr};
use crate::infrastructure::traits::IntoActiveModel;

pub async fn create(media_id: i32, log: &LogCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let mut rule_violations = Vec::new();
    if let Some(rating) = log.stars {
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

    let tran = db.begin().await?;

    let mut model = log.into_active_model();
    model.media_id = Set(media_id);
    model.source_id = Set(source.id);
    model.insert(db).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.clone().into_active_model();
        media_am.bot_controllable = sea_orm::Set(bot_controllable);
        media_am.update(db).await?;
    }

    let src_bot_controllable = source.bot_controllable && user.though_bot;
    if src_bot_controllable != source.bot_controllable {
        let mut source_am = source.into_active_model();
        source_am.bot_controllable = Set(src_bot_controllable);
        source_am.update(db).await?;
    }

    update_media_rating(media, &db).await?;

    tran.commit().await?;

    Ok(())
}

pub async fn details(media_id: i32, media_type: MediaType, log_id: i32, user: &CurrentUser, db: &DbConn) -> Result<Log, SrvErr> {
    let log = Logs::find_by_id(log_id).left_join(Media)
        .filter(media::Column::Id.eq(media_id)).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id))
        .one(db).await?;
    if log.is_none() {
        return Err(SrvErr::NotFound)
    }
    Ok(Log::from(&log.unwrap()))
}

pub async fn update(media_id: i32, log_id: i32, log: &LogUpdate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if log.id != log_id {
        return Err(SrvErr::BadRequest("Endpoint log id does not match log id in metadata!".to_string()));
    }

    let mut rule_violations = Vec::new();
    if let Some(stars) = log.stars {
        if stars < 0f32 || stars > 10f32 {
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
    let model = model.unwrap();

    let source = media.find_related(Sources).filter(sources::Column::Name.eq(&log.source)).one(db).await?;
    if source.is_none() {
        return Err(SrvErr::NotFound);
    }
    let source = source.unwrap();

    let old_stars_value = model.stars;

    let mut am: logs::ActiveModel = model.into();
    am.source_id = Set(source.id);
    am.date = Set(log.date);
    am.completed = Set(log.completed);
    am.stars = Set(log.stars);
    am.comment = Set(log.comment.clone());

    let tran = db.begin().await?;

    am.update(db).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.clone().into_active_model();
        media_am.bot_controllable = sea_orm::Set(bot_controllable);
        media_am.update(db).await?;
    }

    let src_bot_controllable = source.bot_controllable && user.though_bot;
    if src_bot_controllable != source.bot_controllable {
        let mut source_am = source.into_active_model();
        source_am.bot_controllable = Set(src_bot_controllable);
        source_am.update(db).await?;
    }

    if log.stars != old_stars_value {
        update_media_rating(media, &db).await?;
    }

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

pub async fn update_media_rating(media: media::Model, db: &DbConn) -> Result<(), SrvErr> {
    #[derive(FromQueryResult)]
    struct AvgSelect {
        pub sum: f32,
        pub count: i64
    }

    let sel = logs::Entity::find().filter(logs::Column::MediaId.eq(media.id))
        .filter(logs::Column::Stars.is_not_null())
        .select_only()
        .column_as(Expr::col(logs::Column::Id).count(), "count")
        .column_as(Expr::col(logs::Column::Stars).sum(), "sum")
        .group_by(logs::Column::MediaId)
        .into_model::<AvgSelect>()
        .one(db).await?;

    let avg;
    if let Some(sel) = sel {
        avg = sel.sum / sel.count as f32;
    } else {
        avg = 0f32;
    }

    let mut am: media::ActiveModel = media.into();
    am.stars = Set(Some(avg));
    am.update(db).await?;

    Ok(())
}