use cruet::Inflector;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, QuerySelect, Set, TransactionTrait};
use entities::{genres, media, media_genres, sea_orm_active_enums};
use entities::prelude::{Genres, Media, MediaGenres};
use views::genres::GenreCreate;
use views::media::MediaType;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, genre: &GenreCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();


    let tran = db.begin().await?;

    // Nothing changed, so the media keeps its bot_controllable flag; dropping `tran` rolls back.
    if !add_genre_to_media(&media, &genre.name, &tran).await? {
        return Ok(false);
    }

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(&tran).await?;
    }

    tran.commit().await?;
    Ok(true)
}

pub async fn index(media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<Vec<String>, SrvErr> {
    let mut select = Genres::find().distinct().inner_join(Media).filter(media::Column::UserId.eq(user.id));
    if let Some(media_type) = media_type {
        select = select.filter(genres::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
            .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()));
    }
    let genres = select.all(db).await?.iter().map(|m| {
        m.name.clone()
    }).collect();

    Ok(genres)
}

pub async fn delete(media_id: i32, genre_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id)
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let media_genre = media.find_related(MediaGenres).filter(media_genres::Column::GenreId.eq(genre_id)).one(db).await?;
    if media_genre.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media_genre = media_genre.unwrap();

    let tran = db.begin().await?;

    let genre = media_genre.find_related(Genres).one(&tran).await?.expect("DB in invalid state!");
    media_genre.delete(&tran).await?;
    if genre.tmdb_id.is_none() {
        let count = genre.find_related(MediaGenres).count(&tran).await?;
        if count == 0 {
            genre.delete(&tran).await?;
        }
    }

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(&tran).await?;
    }

    tran.commit().await?;

    Ok(())
}

pub async fn add_genre_to_media(media: &media::Model, genre: &str, db: &impl ConnectionTrait) -> Result<bool, SrvErr> {
    let name = genre.to_title_case();

    let existing = media.find_related(Genres).filter(genres::Column::Name.eq(&name)).one(db).await?;
    if existing.is_some() {
        return Ok(false);
    }

    let existing_db = genres::Entity::find().filter(genres::Column::Name.eq(&name))
        .filter(genres::Column::Type.eq::<sea_orm_active_enums::MediaType>(media.r#type.clone())).one(db).await?;

    if let Some(existing) = existing_db {
        let rel = media_genres::ActiveModel {
            media_id: Set(media.id),
            genre_id: Set(existing.id),
        };
        rel.insert(db).await?;
    } else {
        let model = genres::ActiveModel {
            id: NotSet,
            tmdb_id: NotSet,
            name: Set(name),
            r#type: Set(media.r#type.clone()),
        };
        let model = model.insert(db).await?;
        let rel = media_genres::ActiveModel {
            media_id: Set(media.id),
            genre_id: Set(model.id)
        };
        rel.insert(db).await?;
    }

    Ok(true)
}


#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Value;
    use crate::test_support;
    use crate::test_support::db::{assert_statement_count, assert_wrote};
    use crate::test_support::fixtures;

    fn movie_media(id: i32, user: &CurrentUser) -> media::Model {
        fixtures::media_model(id, user.id, sea_orm_active_enums::MediaType::Movie)
    }

    fn movie_genre(id: i32, name: &str) -> genres::Model {
        fixtures::genre_model(id, name, sea_orm_active_enums::MediaType::Movie)
    }

    fn genre_create(name: &str) -> GenreCreate {
        GenreCreate { name: name.to_string() }
    }

    #[tokio::test]
    async fn index_returns_genre_names_for_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![movie_genre(1, "Comedy"), movie_genre(2, "Drama")]])
            .into_connection();

        let names = index(Some(MediaType::Movie), &user, &db).await.ok().expect("index should succeed");

        assert_eq!(names, vec!["Comedy".to_string(), "Drama".to_string()]);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(user.id)]);
    }

    #[tokio::test]
    async fn create_returns_false_when_genre_already_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![movie_media(10, &user)]])
            .append_query_results([vec![movie_genre(1, "Comedy")]])
            .into_connection();

        let created = create(10, &genre_create("comedy"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(!created);
    }

    #[tokio::test]
    async fn create_returns_true_when_genre_added_to_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![movie_media(10, &user)]])
            .append_query_results([Vec::<genres::Model>::new(), Vec::<genres::Model>::new(), vec![movie_genre(7, "Comedy")]])
            .append_query_results([vec![fixtures::media_genre_model(10, 7)]])
            .into_connection();

        let created = create(10, &genre_create("comedy"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(created);
        assert_wrote(&test_support::db::log(db), "COMMIT", &[]);
    }

    #[tokio::test]
    async fn create_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        // The user filter is applied in SQL, so the database returns no media row for this user.
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = create(10, &genre_create("Comedy"), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn delete_removes_media_genre_link() {
        test_support::init();
        let user = fixtures::user();
        // A genre with a TMDB id is never deleted, so only the link is removed.
        let genre = genres::Model { tmdb_id: Some(35), ..movie_genre(5, "Comedy") };
        let db = test_support::db::mock()
            .append_query_results([vec![movie_media(10, &user)]])
            .append_query_results([vec![fixtures::media_genre_model(10, 5)]])
            .append_query_results([vec![genre]])
            .append_exec_results([sea_orm::MockExecResult { last_insert_id: 0, rows_affected: 1 }])
            .into_connection();

        let result = delete(10, 5, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "media_genres""#, &[Value::from(10), Value::from(5)]);
    }

    #[tokio::test]
    async fn add_genre_to_media_creates_genre_when_missing() {
        test_support::init();
        let user = fixtures::user();
        let media = movie_media(10, &user);
        let db = test_support::db::mock()
            .append_query_results([Vec::<genres::Model>::new(), Vec::<genres::Model>::new(), vec![movie_genre(7, "Science Fiction")]])
            .append_query_results([vec![fixtures::media_genre_model(10, 7)]])
            .into_connection();

        let added = add_genre_to_media(&media, "science fiction", &db).await.ok().expect("add should succeed");

        assert!(added);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "genres""#, &[Value::from("Science Fiction")]);
        assert_wrote(&log, r#"INSERT INTO "media_genres""#, &[Value::from(10), Value::from(7)]);
    }
}
