use sea_orm::{ActiveModelTrait, DbConn, EntityTrait};
use sea_orm::ActiveValue::Set;
use entities::languages;
use views::infrastructure::traits::IntoActiveModel;
use crate::infrastructure::SrvErr;

pub async fn refresh(db: &DbConn) -> Result<(), SrvErr> {
    let languages = integrations::tmdb::configuration::languages().await?;
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
    Ok(())
}