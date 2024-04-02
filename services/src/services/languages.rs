use sea_orm::{DbConn, EntityTrait};
use entities::prelude::Languages;
use views::languages::LanguageIndex;
use crate::infrastructure::SrvErr;

pub async fn index(db: &DbConn) -> Result<Vec<LanguageIndex>, SrvErr> {
    let db_langs = Languages::find().all(db).await?;
    let mut languages = Vec::with_capacity(db_langs.len());
    for db_lang in db_langs {
        languages.push(LanguageIndex {
            iso_639_1: db_lang.iso6391,
            english_name: db_lang.english_name,
        });
    }
    Ok(languages)
}