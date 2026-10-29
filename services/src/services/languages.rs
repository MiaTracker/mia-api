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


#[cfg(test)]
mod tests {
    use super::*;
    use entities::languages;
    use crate::test_support;
    use crate::test_support::fixtures;

    #[tokio::test]
    async fn index_returns_all_languages_sorted_as_queried() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![
                languages::Model { iso6391: "de".into(), english_name: "German".into(), name: "Deutsch".into() },
                fixtures::language_model("en", "English"),
            ]])
            .into_connection();

        let result = index(&db).await.ok().expect("index should succeed");

        let pairs: Vec<(&str, &str)> = result.iter().map(|l| (l.iso_639_1.as_str(), l.english_name.as_str())).collect();
        assert_eq!(pairs, vec![("de", "German"), ("en", "English")]);
    }
}
