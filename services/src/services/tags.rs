use cruet::Inflector;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, TransactionTrait};
use sea_orm::ActiveValue::Set;
use entities::prelude::{MediaTags, Tags};
use entities::{media, media_tags, sea_orm_active_enums, tags};
use views::media::MediaType;
use views::tags::TagCreate;
use views::users::CurrentUser;
use crate::infrastructure::SrvErr;

pub async fn create(media_id: i32, tag: TagCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<bool, SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let name = if tag.name.chars().any(|c| c.is_lowercase()) {
        tag.name.to_title_case()
    } else { tag.name };

    let existing = media.find_related(Tags).filter(tags::Column::Name.eq(&name)).one(db).await?;
    if existing.is_some() {
        return Ok(false);
    }

    let existing_db = tags::Entity::find().filter(tags::Column::Name.eq(&name)).one(db).await?;

    let tran = db.begin().await?;

    if let Some(existing) = existing_db {
        let rel = media_tags::ActiveModel {
            media_id: Set(media_id),
            tag_id: Set(existing.id),
        };
        rel.insert(&tran).await?;
    } else {
        let model = tags::ActiveModel {
            id: NotSet,
            name: Set(name),
        };
        let model = model.insert(&tran).await?;
        let rel = media_tags::ActiveModel {
            media_id: Set(media_id),
            tag_id: Set(model.id),
        };
        rel.insert(&tran).await?;
    }

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = sea_orm::Set(bot_controllable);
        media_am.update(&tran).await?;
    }

    tran.commit().await?;
    Ok(true)
}

pub async fn delete(media_id: i32, tag_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id)
        .filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;
    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let media_tag = media.find_related(MediaTags).filter(media_tags::Column::TagId.eq(tag_id)).one(db).await?;

    if media_tag.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media_tag = media_tag.unwrap();

    let tran = db.begin().await?;

    let tag = media_tag.find_related(Tags).one(&tran).await?.expect("DB in invalid state!");
    media_tag.delete(&tran).await?;
    let count = tag.find_related(MediaTags).count(&tran).await?;
    if count == 0 {
        tag.delete(&tran).await?;
    }

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.into_active_model();
        media_am.bot_controllable = sea_orm::Set(bot_controllable);
        media_am.update(&tran).await?;
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

    fn tag_create(name: &str) -> TagCreate {
        TagCreate { name: name.to_string() }
    }

    #[tokio::test]
    async fn create_adds_new_tag_and_link_when_tag_does_not_exist() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<tags::Model>::new()])
            .append_query_results([Vec::<tags::Model>::new()])
            .append_query_results([vec![fixtures::tag_model(7, "Favourite")]])
            .append_query_results([vec![fixtures::media_tag_model(10, 7)]])
            .into_connection();

        let created = create(10, tag_create("favourite"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(created);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "tags""#, &[Value::from("Favourite")]);
        assert_wrote(&log, r#"INSERT INTO "media_tags""#, &[Value::from(10), Value::from(7)]);
    }

    #[tokio::test]
    async fn create_links_existing_tag_when_tag_exists_on_other_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<tags::Model>::new()])
            .append_query_results([vec![fixtures::tag_model(7, "Favourite")]])
            .append_query_results([vec![fixtures::media_tag_model(10, 7)]])
            .into_connection();

        let created = create(10, tag_create("Favourite"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(created);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "media_tags""#, &[Value::from(10), Value::from(7)]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "tags""#);
    }

    #[tokio::test]
    async fn create_returns_false_when_tag_already_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::tag_model(7, "Favourite")]])
            .into_connection();

        let created = create(10, tag_create("favourite"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(!created);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn create_keeps_name_unchanged_when_it_has_no_lowercase_letters() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<tags::Model>::new()])
            .append_query_results([Vec::<tags::Model>::new()])
            .append_query_results([vec![fixtures::tag_model(7, "SCI-FI")]])
            .append_query_results([vec![fixtures::media_tag_model(10, 7)]])
            .into_connection();

        let created = create(10, tag_create("SCI-FI"), MediaType::Movie, &user, &db).await.ok().expect("create should succeed");

        assert!(created);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "tags""#, &[Value::from("SCI-FI")]);
    }

    #[tokio::test]
    async fn create_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = create(10, tag_create("Favourite"), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn delete_removes_link_and_orphaned_tag_when_no_other_media_uses_it() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::media_tag_model(10, 7)]])
            .append_query_results([vec![fixtures::tag_model(7, "Favourite")]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_exec_results([deleted(), deleted()])
            .into_connection();

        let result = delete(10, 7, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "media_tags""#, &[Value::from(10), Value::from(7)]);
        assert_wrote(&log, r#"DELETE FROM "tags""#, &[Value::from(7)]);
    }

    #[tokio::test]
    async fn delete_keeps_tag_when_other_media_still_uses_it() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::media_tag_model(10, 7)]])
            .append_query_results([vec![fixtures::tag_model(7, "Favourite")]])
            .append_query_results([[fixtures::count_row(1)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete(10, 7, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "media_tags""#, &[Value::from(10), Value::from(7)]);
        assert_no_statement_starting_with(&log, r#"DELETE FROM "tags""#);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = delete(10, 7, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_tag_is_not_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<media_tags::Model>::new()])
            .into_connection();

        let result = delete(10, 7, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }
}
