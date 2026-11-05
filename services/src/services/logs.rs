use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbConn, EntityTrait, FromQueryResult, IntoActiveModel as SOIntoActiveModel, JoinType, ModelTrait, QueryFilter, QuerySelect, RelationTrait, TransactionTrait};
use sea_orm::prelude::Expr;
use entities::{logs, media, sea_orm_active_enums, sources};
use entities::prelude::{Logs, Sources};
use views::logs::{Log, LogCreate, LogUpdate};
use views::media::MediaType;
use views::users::CurrentUser;
use crate::infrastructure::{RuleViolation, SrvErr};
use crate::infrastructure::traits::IntoActiveModel;

pub async fn create(media_id: i32, log: &LogCreate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let mut rule_violations = Vec::new();
    if let Some(rating) = log.stars
        && (!(0f32..=10f32).contains(&rating)) {
            rule_violations.push(RuleViolation::LogRatingOutOfBounds);
        }

    if !rule_violations.is_empty() {
        return Err(SrvErr::RuleViolation(rule_violations));
    }

    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let source = media.find_related(Sources).filter(sources::Column::Name.eq(&log.source)).one(db).await?;
    if source.is_none() {
        return Err(SrvErr::NotFound);
    }
    let source = source.unwrap();

    let tran = db.begin().await?;

    let mut model = log.into_active_model();
    model.media_id = Set(media_id);
    model.source_id = Set(source.id);
    model.insert(&tran).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.clone().into_active_model();
        media_am.bot_controllable = sea_orm::Set(bot_controllable);
        media_am.update(&tran).await?;
    }

    let src_bot_controllable = source.bot_controllable && user.though_bot;
    if src_bot_controllable != source.bot_controllable {
        let mut source_am = source.into_active_model();
        source_am.bot_controllable = Set(src_bot_controllable);
        source_am.update(&tran).await?;
    }

    update_media_rating(media, &tran).await?;

    tran.commit().await?;

    Ok(())
}

pub async fn details(media_id: i32, media_type: MediaType, log_id: i32, user: &CurrentUser, db: &DbConn) -> Result<Log, SrvErr> {
    let found = Logs::find_by_id(log_id).find_also_related(Sources)
        .join(JoinType::InnerJoin, logs::Relation::Media.def())
        .filter(media::Column::Id.eq(media_id)).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id))
        .one(db).await?;
    let Some((log, source)) = found else {
        return Err(SrvErr::NotFound)
    };
    let mut view = Log::from(&log);
    if let Some(source) = source {
        view.source = source.name;
    }
    Ok(view)
}

pub async fn update(media_id: i32, log_id: i32, log: &LogUpdate, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    if log.id != log_id {
        return Err(SrvErr::BadRequest("Endpoint log id does not match log id in metadata!".to_string()));
    }

    let mut rule_violations = Vec::new();
    if let Some(stars) = log.stars
        && (!(0f32..=10f32).contains(&stars)) {
            rule_violations.push(RuleViolation::LogRatingOutOfBounds);
        }

    if !rule_violations.is_empty() {
        return Err(SrvErr::RuleViolation(rule_violations));
    }

    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let model = media.find_related(Logs).filter(logs::Column::Id.eq(log_id)).one(db).await?;
    if model.is_none() {
        return Err(SrvErr::NotFound);
    }
    let model = model.unwrap();

    let source = media.find_related(Sources).filter(sources::Column::Name.eq(&log.source)).one(db).await?;
    if source.is_none() {
        return Err(SrvErr::NotFound);
    }
    let source = source.unwrap();

    let old_stars_value = model.stars;

    let mut am: logs::ActiveModel = model.into();
    am.source_id = Set(source.id);
    am.date = Set(log.date);
    am.completed = Set(log.completed);
    am.stars = Set(log.stars);
    am.comment = Set(log.comment.clone());

    let tran = db.begin().await?;

    am.update(&tran).await?;

    let bot_controllable = media.bot_controllable && user.though_bot;
    if bot_controllable != media.bot_controllable {
        let mut media_am = media.clone().into_active_model();
        media_am.bot_controllable = sea_orm::Set(bot_controllable);
        media_am.update(&tran).await?;
    }

    let src_bot_controllable = source.bot_controllable && user.though_bot;
    if src_bot_controllable != source.bot_controllable {
        let mut source_am = source.into_active_model();
        source_am.bot_controllable = Set(src_bot_controllable);
        source_am.update(&tran).await?;
    }

    if log.stars != old_stars_value {
        update_media_rating(media, &tran).await?;
    }

    tran.commit().await?;

    Ok(())
}

pub async fn delete(media_id: i32, log_id: i32, media_type: MediaType, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let media = media::Entity::find_by_id(media_id).filter(media::Column::Type.eq::<sea_orm_active_enums::MediaType>(media_type.into()))
        .filter(media::Column::UserId.eq(user.id)).one(db).await?;

    if media.is_none() {
        return Err(SrvErr::NotFound);
    }
    let media = media.unwrap();

    let log = media.find_related(Logs).filter(logs::Column::Id.eq(log_id)).one(db).await?;
    if log.is_none() {
        return Err(SrvErr::NotFound);
    }
    let log = log.unwrap();

    let tran = db.begin().await?;

    log.delete(&tran).await?;

    update_media_rating(media, &tran).await?;

    tran.commit().await?;

    Ok(())
}

pub async fn update_media_rating(media: media::Model, db: &impl ConnectionTrait) -> Result<(), SrvErr> {
    #[derive(FromQueryResult)]
    struct AvgSelect {
        pub sum: f32,
        pub count: i64
    }

    let sel = logs::Entity::find().filter(logs::Column::MediaId.eq(media.id))
        .filter(logs::Column::Stars.is_not_null())
        .select_only()
        .column_as(Expr::col(logs::Column::Id).count(), "count")
        .column_as(Expr::col(logs::Column::Stars).sum(), "sum")
        .group_by(logs::Column::MediaId)
        .into_model::<AvgSelect>()
        .one(db).await?;

    let avg;
    if let Some(sel) = sel {
        avg = Some(sel.sum / sel.count as f32);
    } else {
        avg = None;
    }

    let mut am: media::ActiveModel = media.into();
    am.stars = Set(avg);
    am.update(db).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::Value;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted};
    use crate::test_support::fixtures;

    fn log_create(stars: Option<f32>) -> LogCreate {
        LogCreate { date: test_support::date(2024, 1, 1), stars, completed: true, comment: Some("Great".to_string()), source: "Torrent A".to_string() }
    }

    fn log_update(id: i32, stars: Option<f32>) -> LogUpdate {
        LogUpdate { id, date: test_support::date(2024, 1, 1), stars, completed: true, comment: Some("Great".to_string()), source: "Torrent A".to_string() }
    }

    fn assert_out_of_bounds(result: Result<(), SrvErr>) {
        match result {
            Err(SrvErr::RuleViolation(violations)) => {
                assert_eq!(violations.len(), 1);
                assert!(matches!(violations[0], RuleViolation::LogRatingOutOfBounds));
            }
            _ => panic!("expected a LogRatingOutOfBounds rule violation"),
        }
    }

    #[tokio::test]
    async fn create_inserts_log_and_updates_media_rating() {
        test_support::init();
        let user = fixtures::user();
        let media = fixtures::movie_media_model(10, user.id);
        let db = test_support::db::mock()
            .append_query_results([vec![media.clone()]])
            .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([vec![fixtures::log_model(1, 10, 3)]])
            .append_query_results([[fixtures::rating_row(8.0, 1)]])
            .append_query_results([vec![media::Model { stars: Some(8.0), ..media }]])
            .into_connection();

        let result = create(10, &log_create(Some(8.0)), MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "logs""#, &[Value::from(10), Value::from(3), Value::from(8.0f32), Value::from("Great")]);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(8.0f32), Value::from(10)]);
    }

    #[tokio::test]
    async fn create_accepts_stars_at_both_bounds() {
        test_support::init();
        let user = fixtures::user();
        for stars in [0.0f32, 10.0f32] {
            let media = fixtures::movie_media_model(10, user.id);
            let db = test_support::db::mock()
                .append_query_results([vec![media.clone()]])
                .append_query_results([vec![fixtures::source_model(3, 10, "Torrent A")]])
                .append_query_results([vec![fixtures::log_model(1, 10, 3)]])
                .append_query_results([[fixtures::rating_row(stars, 1)]])
                .append_query_results([vec![media]])
                .into_connection();

            let result = create(10, &log_create(Some(stars)), MediaType::Movie, &user, &db).await;

            assert!(result.is_ok(), "stars {stars} should be accepted");
        }
    }

    #[tokio::test]
    async fn create_returns_rule_violation_when_stars_are_above_ten() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock().into_connection();

        let result = create(10, &log_create(Some(10.5)), MediaType::Movie, &user, &db).await;

        assert_out_of_bounds(result);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 0);
    }

    #[tokio::test]
    async fn create_returns_rule_violation_when_stars_are_negative() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock().into_connection();

        let result = create(10, &log_create(Some(-0.5)), MediaType::Movie, &user, &db).await;

        assert_out_of_bounds(result);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 0);
    }

    #[tokio::test]
    async fn create_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = create(10, &log_create(None), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn create_returns_not_found_when_source_does_not_exist_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<sources::Model>::new()])
            .into_connection();

        let result = create(10, &log_create(None), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, "SELECT", &[Value::from("Torrent A")]);
    }

    #[tokio::test]
    async fn details_returns_log_when_it_belongs_to_users_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([[(
                logs::Model { stars: Some(7.5), comment: Some("Fine".to_string()), ..fixtures::log_model(1, 10, 3) },
                Some(fixtures::source_model(3, 10, "Torrent A")),
            )]])
            .into_connection();

        let found = details(10, MediaType::Movie, 1, &user, &db).await.expect("details should succeed");

        assert_eq!(found.id, 1);
        assert_eq!(found.source, "Torrent A");
        assert_eq!(found.date, test_support::date(2024, 1, 1));
        assert_eq!(found.stars, Some(7.5));
        assert!(found.completed);
        assert_eq!(found.comment.as_deref(), Some("Fine"));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
    }

    #[tokio::test]
    async fn details_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        // Ownership is part of the single joined query, so another user's log yields no row.
        let db = test_support::db::mock()
            .append_query_results([Vec::<(logs::Model, Option<sources::Model>)>::new()])
            .into_connection();

        let result = details(10, MediaType::Movie, 1, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(1), Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn update_overwrites_log_and_updates_media_rating_when_stars_change() {
        test_support::init();
        let user = fixtures::user();
        let media = fixtures::movie_media_model(10, user.id);
        let existing = logs::Model { stars: Some(5.0), ..fixtures::log_model(1, 10, 3) };
        let db = test_support::db::mock()
            .append_query_results([vec![media.clone()]])
            .append_query_results([vec![existing.clone()]])
            .append_query_results([vec![fixtures::source_model(4, 10, "Torrent A")]])
            .append_query_results([vec![logs::Model { stars: Some(9.0), source_id: 4, ..existing }]])
            .append_query_results([[fixtures::rating_row(9.0, 1)]])
            .append_query_results([vec![media::Model { stars: Some(9.0), ..media }]])
            .into_connection();

        let result = update(10, 1, &log_update(1, Some(9.0)), MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "logs""#, &[Value::from(4), Value::from(9.0f32), Value::from("Great"), Value::from(1)]);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(9.0f32), Value::from(10)]);
    }

    #[tokio::test]
    async fn update_leaves_media_rating_alone_when_stars_are_unchanged() {
        test_support::init();
        let user = fixtures::user();
        let existing = logs::Model { stars: Some(9.0), ..fixtures::log_model(1, 10, 3) };
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![existing.clone()]])
            .append_query_results([vec![fixtures::source_model(4, 10, "Torrent A")]])
            .append_query_results([vec![logs::Model { source_id: 4, ..existing }]])
            .into_connection();

        let result = update(10, 1, &log_update(1, Some(9.0)), MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "logs""#, &[Value::from(4), Value::from(1)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "media""#);
    }

    #[tokio::test]
    async fn update_returns_bad_request_when_endpoint_id_differs_from_body_id() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock().into_connection();

        let result = update(10, 1, &log_update(2, None), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::BadRequest(_))));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 0);
    }

    #[tokio::test]
    async fn update_returns_rule_violation_when_stars_are_above_ten() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock().into_connection();

        let result = update(10, 1, &log_update(1, Some(10.5)), MediaType::Movie, &user, &db).await;

        assert_out_of_bounds(result);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 0);
    }

    #[tokio::test]
    async fn update_returns_rule_violation_when_stars_are_negative() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock().into_connection();

        let result = update(10, 1, &log_update(1, Some(-1.0)), MediaType::Movie, &user, &db).await;

        assert_out_of_bounds(result);
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

        let result = update(10, 1, &log_update(1, None), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn update_returns_not_found_when_log_is_not_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<logs::Model>::new()])
            .into_connection();

        let result = update(10, 1, &log_update(1, None), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn update_returns_not_found_when_source_does_not_exist_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([vec![fixtures::log_model(1, 10, 3)]])
            .append_query_results([Vec::<sources::Model>::new()])
            .into_connection();

        let result = update(10, 1, &log_update(1, None), MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 3);
    }

    #[tokio::test]
    async fn delete_removes_log_and_updates_media_rating() {
        test_support::init();
        let user = fixtures::user();
        let media = fixtures::movie_media_model(10, user.id);
        let db = test_support::db::mock()
            .append_query_results([vec![media.clone()]])
            .append_query_results([vec![fixtures::log_model(1, 10, 3)]])
            .append_query_results([fixtures::no_rating_rows()])
            .append_query_results([vec![media]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete(10, 1, MediaType::Movie, &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"DELETE FROM "logs""#, &[Value::from(1)]);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::Float(None), Value::from(10)]);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_media_belongs_to_another_user() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = delete(10, 1, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[Value::from(10), Value::from(user.id)]);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_log_is_not_on_media() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::movie_media_model(10, user.id)]])
            .append_query_results([Vec::<logs::Model>::new()])
            .into_connection();

        let result = delete(10, 1, MediaType::Movie, &user, &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn update_media_rating_sets_stars_to_average_of_rated_logs() {
        test_support::init();
        let media = fixtures::movie_media_model(10, 1);
        let db = test_support::db::mock()
            .append_query_results([[fixtures::rating_row(17.0, 2)]])
            .append_query_results([vec![media::Model { stars: Some(8.5), ..media.clone() }]])
            .into_connection();

        let result = update_media_rating(media, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::from(8.5f32), Value::from(10)]);
    }

    #[tokio::test]
    async fn update_media_rating_clears_stars_when_no_rated_logs() {
        test_support::init();
        let media = media::Model { stars: Some(7.0), ..fixtures::movie_media_model(10, 1) };
        let db = test_support::db::mock()
            .append_query_results([fixtures::no_rating_rows()])
            .append_query_results([vec![media::Model { stars: None, ..media.clone() }]])
            .into_connection();

        let result = update_media_rating(media, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_wrote(&log, r#"UPDATE "media""#, &[Value::Float(None), Value::from(10)]);
    }
}
