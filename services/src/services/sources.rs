use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, NotSet, PaginatorTrait, QueryFilter, Set, TransactionTrait};
use entities::{media, sea_orm_active_enums, sources};
use entities::prelude::Sources;
use views::media::MediaType;
use views::sources::{SourceCreate, SourceUpdate};
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, source: &SourceCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let source_exists = media.find_related(Sources).filter(sources::Column::Name.eq(&source.name)).count(db).await? >= 1;

    if source_exists {
        return Err(SrvErr::Conflict("A source with this name already exists.".to_string()));
    }


    let model = sources::ActiveModel {
        id: NotSet,
        media_id: Set(media_id),
        name: Set(source.name.clone()),
        url: Set(source.url.clone()),
        r#type: Set((&source.r#type).into()),
    };

    let tran = db.begin().await?;

    model.insert(db).await?;

    tran.commit().await?;

    Ok(())
}

pub async fn update(media_id: i32, source_id: i32, source: &SourceUpdate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if(source.id != source_id) {
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
        return Err(SrvErr::Conflict("A source with this name already exists.".to_string()));
    }

    let mut model: sources::ActiveModel = model.unwrap().into();
    model.name = Set(source.name.clone());
    model.url = Set(source.url.clone());
    model.r#type = Set((&source.r#type).into());

    let tran = db.begin().await?;

    model.update(db).await?;

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

    let tran = db.begin().await?;

    source.delete(db).await?;

    tran.commit().await?;

    Ok(())
}