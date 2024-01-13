use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, QueryFilter, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::{genres, languages};
use entities::sea_orm_active_enums::MediaType;
use crate::infrastructure::SrvErr;
use crate::infrastructure::traits::IntoActiveModel;

pub async fn refresh(db: &DbConn) -> Result<(), SrvErr> {
    let tran = db.begin().await?;
    let languages = integrations::tmdb::services::configuration::languages().await?;
    for language in languages {
        let existing = languages::Entity::find_by_id(language.iso_639_1.clone()).one(db).await?;
        let mut am = language.into_active_model();
        if let Some(model) = existing {
            am.iso6391 = Set(model.iso6391);
            am.update(db).await?;
        } else {
            am.insert(db).await?;
        }
    }

    let movie_genres = integrations::tmdb::services::genres::movie().await?;
    for genre in movie_genres.genres {
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id))
            .filter(genres::Column::Type.eq(MediaType::Movie)).one(db).await?;
        let mut am = genre.into_active_model();
        am.r#type = Set(MediaType::Movie);
        if let Some(model) = existing {
            am.id = Set(model.id);
            am.update(db).await?;
        } else {
            am.insert(db).await?;
        }
    }

    let series_genres = integrations::tmdb::services::genres::tv().await?;
    for genre in series_genres.genres {
        let existing = genres::Entity::find().filter(genres::Column::TmdbId.eq(genre.id))
            .filter(genres::Column::Type.eq(MediaType::Series)).one(db).await?;
        let mut am = genre.into_active_model();
        am.r#type = Set(MediaType::Series);
        if let Some(model) = existing {
            am.id = Set(model.id);
            am.update(db).await?;
        } else {
            am.insert(db).await?;
        }
    }

    tran.commit().await?;
    Ok(())
}