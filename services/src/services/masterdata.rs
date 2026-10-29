use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, QueryFilter, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::{genres, languages};
use entities::sea_orm_active_enums::MediaType;
use crate::infrastructure::{SrvErr, Tmdb};
use crate::infrastructure::traits::IntoActiveModel;

pub async fn refresh(db: &DbConn, tmdb: &dyn Tmdb) -> Result<(), SrvErr> {
    let tran = db.begin().await?;
    let languages = tmdb.languages().await?;
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

    let movie_genres = tmdb.movie_genres().await?;
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

    let series_genres = tmdb.tv_genres().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use integrations::tmdb::views::Languages;
    use sea_orm::{ActiveEnum, Value};
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_wrote};
    use crate::test_support::fixtures;
    use crate::test_support::tmdb::genre_list;
    use crate::test_support::MockTmdb;

    fn english() -> Languages {
        Languages { english_name: "English".into(), iso_639_1: "en".into(), name: "English".into() }
    }

    fn tmdb_genre(id: i32, tmdb_id: i32, name: &str, media_type: MediaType) -> genres::Model {
        genres::Model { tmdb_id: Some(tmdb_id), ..fixtures::genre_model(id, name, media_type) }
    }

    /// A `MockTmdb` answering each masterdata endpoint exactly once.
    fn masterdata_tmdb(languages: Vec<Languages>, movie_genres: &[(i32, &str)], tv_genres: &[(i32, &str)]) -> MockTmdb {
        let movie_genres = genre_list(movie_genres);
        let tv_genres = genre_list(tv_genres);
        let mut tmdb = MockTmdb::new();
        tmdb.expect_languages().times(1).returning(move || Ok(languages.clone()));
        tmdb.expect_movie_genres().times(1).returning(move || Ok(movie_genres.clone()));
        tmdb.expect_tv_genres().times(1).returning(move || Ok(tv_genres.clone()));
        tmdb
    }

    #[tokio::test]
    async fn refresh_inserts_missing_languages_and_genres() {
        test_support::init();
        let tmdb = masterdata_tmdb(vec![english()], &[(28, "Action")], &[(10759, "Action & Adventure")]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<languages::Model>::new()])
            .append_query_results([vec![fixtures::language_model("en", "English")]])
            .append_query_results([Vec::<genres::Model>::new()])
            .append_query_results([vec![tmdb_genre(1, 28, "Action", MediaType::Movie)]])
            .append_query_results([Vec::<genres::Model>::new()])
            .append_query_results([vec![tmdb_genre(2, 10759, "Action & Adventure", MediaType::Series)]])
            .into_connection();

        let result = refresh(&db, &tmdb).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "languages""#, &[Value::from("en"), Value::from("English")]);
        assert_wrote(&log, r#"INSERT INTO "genres""#, &[Value::from(28), Value::from("Action"), MediaType::Movie.into_value().into()]);
        assert_wrote(&log, r#"INSERT INTO "genres""#, &[Value::from(10759), Value::from("Action & Adventure"), MediaType::Series.into_value().into()]);
        assert_no_statement_starting_with(&log, "UPDATE");
    }

    #[tokio::test]
    async fn refresh_updates_existing_languages_and_genres_in_place_when_already_stored() {
        test_support::init();
        let tmdb = masterdata_tmdb(vec![english()], &[(28, "Action")], &[]);
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::language_model("en", "English")]])
            .append_query_results([vec![fixtures::language_model("en", "English")]])
            .append_query_results([vec![tmdb_genre(5, 28, "Old Action", MediaType::Movie)]])
            .append_query_results([vec![tmdb_genre(5, 28, "Action", MediaType::Movie)]])
            .into_connection();

        let result = refresh(&db, &tmdb).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_no_statement_starting_with(&log, "INSERT");
        assert_wrote(&log, r#"UPDATE "languages""#, &[Value::from("en"), Value::from("English")]);
        assert_wrote(&log, r#"UPDATE "genres""#, &[Value::from(5), Value::from(28), Value::from("Action")]);
    }

    #[tokio::test]
    async fn refresh_returns_integration_error_when_tmdb_fails() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_languages().times(1).returning(|| Err(test_support::tmdb::error("TMDB unavailable")));
        let db = test_support::db::mock().into_connection();

        let result = refresh(&db, &tmdb).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
    }
}