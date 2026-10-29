use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel, ModelTrait, NotSet, PaginatorTrait, QueryFilter, Set, TransactionTrait};
use entities::{media, sea_orm_active_enums, sources};
use entities::prelude::{Logs, Media, Sources};
use views::media::MediaType;
use views::sources::{Source, SourceCreate, SourceUpdate};
use views::users::CurrentUser;
use crate::infrastructure::{RuleViolation, SrvErr};
use crate::logs::update_media_rating;

pub async fn create(media_id: i32, source: &SourceCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    
    //TODO: validate source.name is not blank

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

    let has_logs = source.find_related(Logs).count(db).await? > 0;
    if has_logs {
        return Err(SrvErr::RuleViolation(vec![RuleViolation::SourceHasLogs]));
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


#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Value;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted};
    use crate::test_support::assert_single_violation;
    use crate::test_support::fixtures;
    use views::sources::SourceType;

    fn source_create(name: &str) -> SourceCreate {
        SourceCreate { name: name.to_string(), url: "https://example.test/new".to_string(), r#type: SourceType::Torrent }
    }

    fn source_update(id: i32, name: &str) -> SourceUpdate {
        SourceUpdate { id, name: name.to_string(), url: "https://example.test/changed".to_string(), r#type: SourceType::Jellyfin }
    }

    #[tokio::test]
    async fn create_inserts_source_when_name_is_unused() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .into_connection();

        let result = create(10, &source_create("Torrent A"), MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "sources""#, &[
            Value::from(10),
            Value::from("Torrent A"),
            Value::from("https://example.test/new"),
            Value::from(false),
        ]);
    }

    #[tokio::test]
    async fn create_marks_source_bot_controllable_when_user_is_a_bot() {
        test_support::init();
        let user = fixtures::bot_user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .into_connection();

        let result = create(10, &source_create("Torrent A"), MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "sources""#, &[Value::from("Torrent A"), Value::from(true)]);
    }

    #[tokio::test]
    async fn create_returns_rule_violation_when_source_name_already_exists() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();

        let result = create(10, &source_create("Torrent A"), MediaType::Movie, &user, &db).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SourceNameAlreadyExists));
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

        let result = create(10, &source_create("Torrent A"), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn index_returns_sources_of_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A"), fixtures::source_model(4, 10, "Torrent B")]])
            .into_connection();

        let sources = index(10, MediaType::Movie, &user, &db).await.ok().expect("index should succeed");

        assert_eq!(sources.iter().map(|s| (s.id, s.name.as_str())).collect::<Vec<_>>(), vec![(3, "Torrent A"), (4, "Torrent B")]);
        assert_eq!(sources[0].url, "https://source-3.example.test");
        assert!(matches!(sources[0].r#type, SourceType::Web));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn index_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = index(10, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn details_returns_source_when_it_belongs_to_users_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .into_connection();

        let source = details(10, MediaType::Movie, 3, &user, &db).await.ok().expect("details should succeed");

        assert_eq!(source.id, 3);
        assert_eq!(source.name, "Torrent A");
        assert_eq!(source.url, "https://source-3.example.test");
        assert!(matches!(source.r#type, SourceType::Web));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
    }

    #[tokio::test]
    async fn details_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        // Ownership is part of the single joined query, so another user's source yields no row.
        let db = test_support::db::mock()
            .append_query_results([Vec::<sources::Model>::new()])
            .into_connection();

        let result = details(10, MediaType::Movie, 3, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(3), Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn update_overwrites_source_fields_when_name_is_free() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent B")]])
            .into_connection();

        let result = update(10, 3, &source_update(3, "Torrent B"), MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "sources""#, &[Value::from("Torrent B"), Value::from("https://example.test/changed"), Value::from(3)]);
    }

    #[tokio::test]
    async fn update_returns_bad_request_when_endpoint_id_differs_from_body_id() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock().into_connection();

        let result = update(10, 3, &source_update(4, "Torrent B"), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(_))));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 0);
    }

    #[tokio::test]
    async fn update_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = update(10, 3, &source_update(3, "Torrent B"), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn update_returns_not_found_when_source_is_not_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<sources::Model>::new()])
            .into_connection();

        let result = update(10, 3, &source_update(3, "Torrent B"), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn update_returns_rule_violation_when_name_belongs_to_another_source() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();

        let result = update(10, 3, &source_update(3, "Torrent B"), MediaType::Movie, &user, &db).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SourceNameAlreadyExists));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
    }

    #[tokio::test]
    async fn delete_removes_source_and_recalculates_media_rating() {
        test_support::init();
        let user = fixtures::user();
        let media = fixtures::movie_media_model(10, user.id);
        let db = test_support::db::mock()
            .append_query_results([vec![media.clone()]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([fixtures::no_rating_rows()])
            .append_query_results([vec![media]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete(10, 3, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "sources""#, &[Value::from(3)]);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::Float(None), Value::from(10)]);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = delete(10, 3, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_source_is_not_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<sources::Model>::new()])
            .into_connection();

        let result = delete(10, 3, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn delete_returns_rule_violation_when_source_has_logs() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::count_row(2)]])
            .into_connection();

        let result = delete(10, 3, MediaType::Movie, &user, &db).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SourceHasLogs));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
        assert_no_statement_starting_with(&log, r#"DELETE FROM "sources""#);
    }

    #[tokio::test]
    async fn delete_from_media_removes_source_when_it_has_no_logs() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(0)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete_from_media(fixtures::movie_media_model(10, user.id), fixtures::source_model(3, 10, "Torrent A"), &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "sources""#, &[Value::from(3)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "media""#);
    }

    #[tokio::test]
    async fn delete_from_media_removes_source_when_bot_controls_it() {
        test_support::init();
        let user = fixtures::bot_user();
        let source = sources::Model { bot_controllable: true, ..fixtures::source_model(3, 10, "Torrent A") };
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(0)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete_from_media(fixtures::movie_media_model(10, user.id), source, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "sources""#, &[Value::from(3)]);
    }

    #[tokio::test]
    async fn delete_from_media_revokes_bot_control_of_media_when_user_is_not_a_bot() {
        test_support::init();
        let user = fixtures::user();
        let media = media::Model { bot_controllable: true, ..fixtures::movie_media_model(10, user.id) };
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([vec![media::Model { bot_controllable: false, ..media.clone() }]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete_from_media(media, fixtures::source_model(3, 10, "Torrent A"), &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(false), Value::from(10)]);
    }

    #[tokio::test]
    async fn delete_from_media_returns_unauthorized_when_bot_deletes_source_it_does_not_control() {
        test_support::init();
        let user = fixtures::bot_user();
        let db = test_support::db::mock().into_connection();

        let result = delete_from_media(fixtures::movie_media_model(10, user.id), fixtures::source_model(3, 10, "Torrent A"), &user, &db).await;

        assert!(matches!(result, Err(SrvErr::Unauthorized)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 0);
    }

    #[tokio::test]
    async fn delete_from_media_returns_rule_violation_when_source_has_logs() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();

        let result = delete_from_media(fixtures::movie_media_model(10, user.id), fixtures::source_model(3, 10, "Torrent A"), &user, &db).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SourceHasLogs));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
    }
}
