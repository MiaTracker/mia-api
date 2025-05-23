use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, Set, TransactionTrait};
use entities::{media, sea_orm_active_enums, sources};
use entities::prelude::{Media, Sources};
use views::media::MediaType;
use views::sources::{Source, SourceCreate, SourceUpdate};
use views::users::CurrentUser;
use crate::infrastructure::{RuleViolation, SrvErr};
use crate::logs::update_media_rating;

pub async fn create(media_id: i32, source: &SourceCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let source_exists = media.find_related(Sources).filter(sources::Column::Name.eq(&source.name)).count(db).await? >= 1;

    if source_exists {
        return Err(SrvErr::RuleViolation(vec![RuleViolation::SourceNameAlreadyExists]));
    }


    let model = sources::ActiveModel {
        id: NotSet,
        media_id: Set(media_id),
        name: Set(source.name.clone()),
        url: Set(source.url.clone()),
        r#type: Set((&source.r#type).into()),
        bot_controllable: Set(user.though_bot)
    };

    let tran = db.begin().await?;

    model.insert(db).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    tran.commit().await?;

    Ok(())
}

pub async fn index(media_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<Vec<Source>, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let sources = media.find_related(Sources).all(db).await?;
    let sources: Vec<Source> = sources.iter().map(|src| {
        Source::from(src)
    }).collect();
    Ok(sources)
}

pub async fn details(media_id: i32, media_type: MediaType, source_id: i32, user: &CurrentUser, db: &DbConn) -> Result<Source, SrvErr> {
    let source = Sources::find_by_id(source_id).left_join(Media)
        .filter(media::Column::Id.eq(media_id)).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id))
        .one(db).await?;
    if source.is_none() {
        return Err(SrvErr::NotFound)
    }
    Ok(Source::from(&source.unwrap()))
}

pub async fn update(media_id: i32, source_id: i32, source: &SourceUpdate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if source.id != source_id {
        return Err(SrvErr::BadRequest("Endpoint source id does not match source id in metadata!".to_string()));
    }

    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let model = media.find_related(Sources).filter(sources::Column::Id.eq(source_id)).one(db).await?;
    if model.is_none() {
        return Err(SrvErr::NotFound);
    }

    let name_conflict = media.find_related(Sources).filter(sources::Column::Name.eq(&source.name)).filter(sources::Column::Id.ne(source_id)).count(db).await? != 0;
    if name_conflict {
        return Err(SrvErr::RuleViolation(vec![RuleViolation::SourceNameAlreadyExists]));
    }

    let mut model: sources::ActiveModel = model.unwrap().into();
    model.name = Set(source.name.clone());
    model.url = Set(source.url.clone());
    model.r#type = Set((&source.r#type).into());

    let tran = db.begin().await?;

    model.update(db).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    tran.commit().await?;

    Ok(())
}

pub async fn delete(media_id: i32, source_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let source = media.find_related(Sources).filter(sources::Column::Id.eq(source_id)).one(db).await?;
    if source.is_none() {
        return Err(SrvErr::NotFound);
    }
    let source = source.unwrap();

    delete_from_media(media.clone(), source, user, db).await?;

    update_media_rating(media, &db).await?;

    Ok(())
}

pub async fn delete_from_media(media: media::Model, source: sources::Model, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if user.though_bot && !source.bot_controllable {
        return Err(SrvErr::Unauthorized)
    }

    let tran = db.begin().await?;

    source.delete(db).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    tran.commit().await?;

    Ok(())
}