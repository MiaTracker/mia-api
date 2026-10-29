use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, Set, TransactionTrait};
use entities::{media, sea_orm_active_enums, titles};
use entities::prelude::Titles;
use views::media::MediaType;
use views::titles::TitleCreate;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, title: &TitleCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let existing = media.find_related(Titles).filter(titles::Column::Title.eq(&title.name)).one(db).await?;
    if existing.is_some() {
        return Ok(false);
    }

    let primary = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).count(db).await? == 0;
    let model = titles::ActiveModel {
        id: NotSet,
        media_id: Set(media_id),
        primary: Set(primary),
        title: Set(title.name.clone()),
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

    Ok(true)
}

pub async fn set_primary(media_id: i32, title_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let title = media.find_related(Titles).filter(titles::Column::Id.eq(title_id)).one(db).await?;
    if title.is_none() {
        return Err(SrvErr::NotFound);
    }

    let tran = db.begin().await?;

    let primary = media.find_related(Titles).filter(titles::Column::Primary.eq(true)).one(db).await?;
    if primary.is_some() {
        let mut model: titles::ActiveModel = primary.unwrap().into();
        model.primary = Set(false);
        model.update(db).await?;
    }

    let mut title: titles::ActiveModel = title.unwrap().into();
    title.primary = Set(true);
    title.update(db).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    tran.commit().await?;

    Ok(())
}

pub async fn delete(media_id: i32, title_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id)
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let title = media.find_related(Titles).filter(titles::Column::Id.eq(title_id)).one(db).await?;
    if title.is_none() {
        return Err(SrvErr::NotFound);
    }
    let title = title.unwrap();

    let tran = db.begin().await?;

    if title.primary {
        let first = media.find_related(Titles).filter(titles::Column::Primary.eq(false)).one(db).await?;
        title.delete(db).await?;
        if first.is_some() {
            let mut model: titles::ActiveModel = first.unwrap().into();
            model.primary = Set(true);
            model.update(db).await?;
        }
    } else {
        title.delete(db).await?;
    }

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = Set(bot_controllable);
        media_am.update(db).await?;
    }

    tran.commit().await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Value;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted};
    use crate::test_support::fixtures;

    fn title_create(name: &str) -> TitleCreate {
        TitleCreate { name: name.to_string() }
    }

    #[tokio::test]
    async fn create_makes_first_title_primary_when_media_has_no_primary_title() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<titles::Model>::new()])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([vec![fixtures::title_model(1, 10, "Alien", true)]])
            .into_connection();

        let created = create(10, &title_create("Alien"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(created);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(10), Value::from(true), Value::from("Alien")]);
    }

    #[tokio::test]
    async fn create_adds_non_primary_title_when_media_already_has_primary_title() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<titles::Model>::new()])
            .append_query_results([[fixtures::count_row(1)]])
            .append_query_results([vec![fixtures::title_model(2, 10, "Aliens", false)]])
            .into_connection();

        let created = create(10, &title_create("Aliens"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(created);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "titles""#, &[Value::from(10), Value::from(false), Value::from("Aliens")]);
    }

    #[tokio::test]
    async fn create_returns_false_when_title_already_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::title_model(1, 10, "Alien", true)]])
            .into_connection();

        let created = create(10, &title_create("Alien"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(!created);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn create_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = create(10, &title_create("Alien"), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn set_primary_unsets_previous_primary_title() {
        test_support::init();
        let user = fixtures::user();
        let old_primary = fixtures::title_model(1, 10, "Alien", true);
        let new_primary = fixtures::title_model(2, 10, "Aliens", false);
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![new_primary.clone()]])
            .append_query_results([vec![old_primary.clone()]])
            .append_query_results([vec![titles::Model { primary: false, ..old_primary }]])
            .append_query_results([vec![titles::Model { primary: true, ..new_primary }]])
            .into_connection();

        let result = set_primary(10, 2, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "titles""#, &[Value::from(false), Value::from(1)]);
        assert_wrote(&log, r#"UPDATE "titles""#, &[Value::from(true), Value::from(2)]);
    }

    #[tokio::test]
    async fn set_primary_marks_title_primary_when_media_has_no_primary_title() {
        test_support::init();
        let user = fixtures::user();
        let title = fixtures::title_model(2, 10, "Aliens", false);
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![title.clone()]])
            .append_query_results([Vec::<titles::Model>::new()])
            .append_query_results([vec![titles::Model { primary: true, ..title }]])
            .into_connection();

        let result = set_primary(10, 2, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "titles""#, &[Value::from(true), Value::from(2)]);
    }

    #[tokio::test]
    async fn set_primary_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = set_primary(10, 2, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn set_primary_returns_not_found_when_title_is_not_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<titles::Model>::new()])
            .into_connection();

        let result = set_primary(10, 2, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn delete_removes_title_when_title_is_not_primary() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::title_model(2, 10, "Aliens", false)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete(10, 2, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "titles""#, &[Value::from(2)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "titles""#);
    }

    #[tokio::test]
    async fn delete_promotes_another_title_when_deleting_the_primary_title() {
        test_support::init();
        let user = fixtures::user();
        let other = fixtures::title_model(2, 10, "Aliens", false);
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::title_model(1, 10, "Alien", true)]])
            .append_query_results([vec![other.clone()]])
            .append_query_results([vec![titles::Model { primary: true, ..other }]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete(10, 1, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "titles""#, &[Value::from(1)]);
        assert_wrote(&log, r#"UPDATE "titles""#, &[Value::from(true), Value::from(2)]);
    }

    #[tokio::test]
    async fn delete_removes_primary_title_without_promotion_when_no_other_title_exists() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::title_model(1, 10, "Alien", true)]])
            .append_query_results([Vec::<titles::Model>::new()])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete(10, 1, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "titles""#, &[Value::from(1)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "titles""#);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = delete(10, 2, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_title_is_not_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<titles::Model>::new()])
            .into_connection();

        let result = delete(10, 2, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }
}
