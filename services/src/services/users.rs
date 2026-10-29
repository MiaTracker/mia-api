use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use fancy_regex::Regex;
use jsonwebtoken::{encode, EncodingKey, Header};
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, IntoActiveModel as SORMIntoActiveModel, ModelTrait, QueryFilter};
use sea_orm::ActiveValue::Set;
use uuid::Uuid;
use entities::prelude::{AppTokens, Users};
use entities::{app_tokens, users};
use once_cell::sync::Lazy;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::Func;
use views::users::{CurrentUser, PasswordChange, TokenClaims, TokenType, UserIndex, UserLogin, UserProfile, UserRegistration, UserToken};
use crate::infrastructure::{Clock, Random, RuleViolation, SrvErr};
use crate::infrastructure::traits::IntoActiveModel;

static PASS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(?=.*[a-z])(?=.*[A-Z])(?=.*\d)[a-zA-Z\d\w\W]{7,}$"#).expect("Invalid password regex!")
});

pub async fn register(user: &UserRegistration, db: &DbConn, random: &dyn Random) -> Result<(), SrvErr> {
    let email_regex = Regex::new(r#"^(?:[A-Za-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*|"(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21\x23-\x5b\x5d-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*")@(?:(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?|\[(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9])\.){3}(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9])|[a-z0-9-]*[a-z0-9]:(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21-\x5a\x53-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])+)])$"#).expect("Regex is invalid!");

    let mut violations = Vec::new();

    if user.email.is_empty() {
        violations.push(RuleViolation::SignUpEmailEmpty);
    }
    if user.username.is_empty() {
        violations.push(RuleViolation::SignUpUsernameEmpty);
    }
    if user.password.is_empty() {
        violations.push(RuleViolation::SignUpPasswordEmpty);
    }
    if user.password_repeat.is_empty() {
        violations.push(RuleViolation::SignUpPasswordRepeatEmpty);
    }

    if !violations.is_empty() {
        return Err(SrvErr::RuleViolation(violations));
    }

    let email_lower = user.email.to_lowercase();
    if let Ok(Some(m)) = email_regex.find(email_lower.as_str()) {
        if m.as_str() != email_lower {
            violations.push(RuleViolation::SignUpEmailInvalid);
        }
    } else {
        violations.push(RuleViolation::SignUpEmailInvalid);
    }

    let result = Users::find().filter(Expr::expr(Func::lower(
        Expr::col((users::Entity, users::Column::Email)))).eq(email_lower))
        .one(db).await?;
    if result.is_some() {
        violations.push(RuleViolation::SignUpAccountWithThisEmailAlreadyExists);
    }

    let result = Users::find().filter(users::Column::Username.eq(user.username.clone())).one(db).await?;
    if result.is_some() {
        violations.push(RuleViolation::SignUpUsernameAlreadyTaken);
    }

    if user.password != user.password_repeat {
        violations.push(RuleViolation::SignUpPasswordsDoNotMatch);
    }

    if let Ok(m) = PASS_REGEX.find(user.password.as_str()) {
        if m.is_none() {
            violations.push(RuleViolation::SignUpPasswordRequirementsNotMet);
        }
    }
    else {
        violations.push(RuleViolation::SignUpPasswordRequirementsNotMet);
    }

    if !violations.is_empty() {
        return Err(SrvErr::RuleViolation(violations));
    }

    let salt = random.salt();
    let hashed_password = match Argon2::default().hash_password(user.password.as_bytes(), &salt) {
        Ok(hash) => { hash }
        Err(err) => {
            return Err(SrvErr::Internal(format!("Error while hashing password: {}", err)));
        }
    }.to_string();

    let mut model = user.into_active_model();
    model.uuid = Set(random.uuid());
    model.password_hash = Set(hashed_password);
    match model.insert(db).await {
        Ok(_) => { Ok(()) }
        Err(db_err) => { Err(SrvErr::DB(db_err)) }
    }
}

pub async fn login(user: &UserLogin, jwt_secret: &String, db: &DbConn, clock: &dyn Clock) -> Result<UserToken, SrvErr> {
    let mut violations = Vec::new();
    if user.username.is_empty() {
        violations.push(RuleViolation::LoginUsernameEmpty);
    }
    if user.password.is_empty() {
        violations.push(RuleViolation::LoginPasswordEmpty);
    }

    if !violations.is_empty() {
        return Err(SrvErr::RuleViolation(violations));
    }

    let model = Users::find().filter(users::Column::Username.eq(user.username.clone())).one(db).await.map_err(|err| SrvErr::DB(err))?;
    if model.is_none() {
        return Err(SrvErr::Unauthorized);
    }
    let model = model.unwrap();

    let valid = match PasswordHash::new(&model.password_hash) {
        Ok(parsed_hash) => { Argon2::default().verify_password(user.password.as_bytes(), &parsed_hash).map_or(false, |_| true ) }
        Err(_) => { false }
    };

    if !valid {
        return Err(SrvErr::Unauthorized);
    }

    let now = clock.now();
    let iat = now.timestamp() as usize;
    let expiry_date = now + chrono::Duration::days(60);
    let exp = expiry_date.timestamp() as usize;
    let claims = TokenClaims {
        sub: model.uuid,
        exp: Some(exp),
        iat: Some(iat),
        r#type: TokenType::UserToken
    };

    let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(jwt_secret.as_ref()))
        .map_err(|err| SrvErr::Internal(format!("Failed to encode token: {}", err)))?;

    let view = UserToken {
        token,
        expiry_date,
        admin: model.admin
    };

    Ok(view)
}

pub async fn query_user_by_uuid(user_id: Uuid, db: &DbConn) -> Result<Option<CurrentUser>, SrvErr> {
    match users::Entity::find().filter(users::Column::Uuid.eq(user_id)).one(db).await {
        Ok(opt) => {
            match opt {
                None => { Ok(None) }
                Some(user) => { Ok(Some(CurrentUser::from(user))) }
            }
        }
        Err(err) => { Err(SrvErr::DB(err)) }
    }
}

pub async fn query_user_by_app_token(token_id: Uuid, db: &DbConn) -> Result<Option<CurrentUser>, SrvErr> {
    let user = Users::find().inner_join(AppTokens)
        .filter(app_tokens::Column::Uuid.eq(token_id)).one(db).await?;

    Ok(user.map(|user| { CurrentUser::from(user) }))
}

pub fn profile(user: &CurrentUser) -> UserProfile {
    UserProfile {
        uuid: user.uuid,
        username: user.username.clone(),
        email: user.email.clone(),
        admin: user.admin,
    }
}

pub async fn index(db: &DbConn) -> Result<Vec<UserIndex>, SrvErr> {
    let users = users::Entity::find().all(db).await?;
    let users = users.iter().map(|u| {
        UserIndex {
            uuid: u.uuid,
            username: u.username.clone(),
            email: u.email.clone(),
            admin: u.admin,
        }
    }).collect();
    Ok(users)
}

pub async fn change_password(pass_change: PasswordChange, user: &CurrentUser, db: &DbConn, random: &dyn Random) -> Result<(), SrvErr> {
    let mut violations = Vec::new();
    if pass_change.old_password.is_empty() {
        violations.push(RuleViolation::PasswordChangeOldPasswordEmpty);
    }
    if pass_change.new_password.is_empty() {
        violations.push(RuleViolation::PasswordChangeNewPasswordEmpty);
    }
    if pass_change.password_repeat.is_empty() {
        violations.push(RuleViolation::PasswordChangePasswordRepeatEmpty);
    }

    if !violations.is_empty() {
        return Err(SrvErr::RuleViolation(violations));
    }

    let model = Users::find_by_id(user.id).one(db).await.map_err(|err| SrvErr::DB(err))?;
    if model.is_none() {
        return Err(SrvErr::Unauthorized);
    }
    let model = model.unwrap();

    let valid = match PasswordHash::new(&model.password_hash) {
        Ok(parsed_hash) => { Argon2::default().verify_password(pass_change.old_password.as_bytes(), &parsed_hash).map_or(false, |_| true ) }
        Err(_) => { false }
    };

    if !valid {
        return Err(SrvErr::Unauthorized);
    }

    if pass_change.new_password != pass_change.password_repeat {
        violations.push(RuleViolation::PasswordChangePasswordsDoNotMatch);
    }


    if let Ok(m) = PASS_REGEX.find(pass_change.new_password.as_str()) {
        if m.is_none() {
            violations.push(RuleViolation::PasswordChangePasswordRequirementsNotMet);
        }
    }
    else {
        violations.push(RuleViolation::PasswordChangePasswordRequirementsNotMet);
    }

    if !violations.is_empty() {
        return Err(SrvErr::RuleViolation(violations));
    }

    let salt = random.salt();
    let hashed_password = match Argon2::default().hash_password(pass_change.new_password.as_bytes(), &salt) {
        Ok(hash) => { hash }
        Err(err) => {
            return Err(SrvErr::Internal(format!("Error while hashing password: {}", err)));
        }
    }.to_string();

    let mut am = model.into_active_model();
    am.password_hash = Set(hashed_password);
    match am.update(db).await {
        Ok(_) => { Ok(()) }
        Err(db_err) => { Err(SrvErr::DB(db_err)) }
    }
}

pub async fn delete(uuid: Uuid, db: &DbConn) -> Result<(), SrvErr> {
    let user = Users::find().filter(users::Column::Uuid.eq(uuid)).one(db).await?;
    if user.is_none() {
        return Err(SrvErr::NotFound);
    }
    let user = user.unwrap();
    user.delete(db).await?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use jsonwebtoken::{decode, DecodingKey, Validation};
    use sea_orm::Value;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted, string_value_written};
    use crate::test_support::fixtures;
    use crate::test_support::{assert_single_violation, fixed_clock, SeqRandom};

    const SALT: &str = "dGVzdHNhbHR0ZXN0c2FsdA";

    fn no_users() -> Vec<users::Model> {
        Vec::new()
    }

    /// Queues what `register` needs when it reaches the insert: no clashing email or username,
    /// then the inserted row.
    fn registering_db() -> sea_orm::DatabaseConnection {
        test_support::db::mock()
            .append_query_results([no_users()])
            .append_query_results([no_users()])
            .append_query_results([vec![fixtures::user_model(1)]])
            .into_connection()
    }

    /// Queues what `register` needs when it only runs the two uniqueness checks.
    fn rejecting_db() -> sea_orm::DatabaseConnection {
        test_support::db::mock()
            .append_query_results([no_users()])
            .append_query_results([no_users()])
            .into_connection()
    }

    #[test]
    fn profile_maps_current_user_fields() {
        test_support::init();
        let user = CurrentUser { admin: true, ..fixtures::user() };

        let view = profile(&user);

        assert_eq!(view.uuid, user.uuid);
        assert_eq!(view.username, "tester");
        assert_eq!(view.email, "tester@example.test");
        assert!(view.admin);
    }

    #[tokio::test]
    async fn query_user_by_uuid_returns_none_when_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<users::Model>::new()])
            .into_connection();

        let result = query_user_by_uuid(Uuid::from_u128(99), &db).await.ok().expect("query should succeed");

        assert!(result.is_none());
        let log = test_support::db::log(db);
        assert_wrote(&log, "SELECT", &[Value::from(Uuid::from_u128(99))]);
    }

    #[tokio::test]
    async fn query_user_by_app_token_returns_owner() {
        test_support::init();
        let token_id = Uuid::from_u128(500);
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::user_model(3)]])
            .into_connection();

        let user = query_user_by_app_token(token_id, &db).await.ok().expect("query should succeed")
            .expect("owner should be found");

        assert_eq!(user.id, 3);
        assert_eq!(user.uuid, Uuid::from_u128(3));
        assert!(!user.though_bot);
        let log = test_support::db::log(db);
        assert_wrote(&log, "SELECT", &[Value::from(token_id)]);
    }

    #[tokio::test]
    async fn index_returns_all_users() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![
                fixtures::user_model(1),
                users::Model { admin: true, ..fixtures::user_model(2) },
            ]])
            .into_connection();

        let result = index(&db).await.ok().expect("index should succeed");

        let rows: Vec<(Uuid, bool)> = result.iter().map(|u| (u.uuid, u.admin)).collect();
        assert_eq!(rows, vec![(Uuid::from_u128(1), false), (Uuid::from_u128(2), true)]);
    }

    #[tokio::test]
    async fn register_stores_uuid_from_random_and_argon2_hash_with_fixed_salt() {
        test_support::init();
        let db = registering_db();

        let result = register(&fixtures::user_registration(), &db, &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, "INSERT INTO \"users\"", &[Value::from(Uuid::from_u128(1000))]);
        let hash = string_value_written(&log, "INSERT INTO \"users\"", "$argon2").expect("a password hash is stored");
        assert!(hash.contains(SALT), "hash {hash} does not use the fixed salt");
        assert_eq!(hash, fixtures::password_hash(fixtures::PASSWORD));
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_email_empty() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let registration = UserRegistration { email: String::new(), ..fixtures::user_registration() };

        let result = register(&registration, &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpEmailEmpty));
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_username_empty() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let registration = UserRegistration { username: String::new(), ..fixtures::user_registration() };

        let result = register(&registration, &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpUsernameEmpty));
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_password_empty() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let registration = UserRegistration { password: String::new(), ..fixtures::user_registration() };

        let result = register(&registration, &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpPasswordEmpty));
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_password_repeat_empty() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let registration = UserRegistration { password_repeat: String::new(), ..fixtures::user_registration() };

        let result = register(&registration, &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpPasswordRepeatEmpty));
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_email_invalid() {
        test_support::init();
        let db = rejecting_db();
        let registration = UserRegistration { email: "not-an-email".into(), ..fixtures::user_registration() };

        let result = register(&registration, &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpEmailInvalid));
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_email_taken() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::user_model(2)]])
            .append_query_results([no_users()])
            .into_connection();

        let result = register(&fixtures::user_registration(), &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpAccountWithThisEmailAlreadyExists));
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_username_taken() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([no_users()])
            .append_query_results([vec![fixtures::user_model(2)]])
            .into_connection();

        let result = register(&fixtures::user_registration(), &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpUsernameAlreadyTaken));
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_passwords_do_not_match() {
        test_support::init();
        let db = rejecting_db();
        let registration = UserRegistration { password_repeat: "Different1".into(), ..fixtures::user_registration() };

        let result = register(&registration, &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpPasswordsDoNotMatch));
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn register_returns_rule_violation_when_password_requirements_not_met() {
        test_support::init();
        let db = rejecting_db();
        let registration = UserRegistration { password: "password".into(), password_repeat: "password".into(), ..fixtures::user_registration() };

        let result = register(&registration, &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::SignUpPasswordRequirementsNotMet));
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn login_issues_token_with_iat_now_and_exp_now_plus_60_days() {
        test_support::init();
        let clock = fixed_clock(2026, 1, 1, 0, 0, 0);
        let model = users::Model { admin: true, ..fixtures::user_model_with_password(1, fixtures::PASSWORD) };
        let db = test_support::db::mock().append_query_results([vec![model.clone()]]).into_connection();
        let login_request = UserLogin { username: "tester".into(), password: fixtures::PASSWORD.into() };

        let view = login(&login_request, &"test-secret".to_string(), &db, &clock).await.ok().expect("login should succeed");

        let mut validation = Validation::default();
        validation.validate_exp = false;
        let claims = decode::<TokenClaims>(&view.token, &DecodingKey::from_secret(b"test-secret"), &validation)
            .expect("token decodes with the secret").claims;
        let now = clock.now();
        assert_eq!(claims.sub, model.uuid);
        assert_eq!(claims.iat, Some(now.timestamp() as usize));
        assert_eq!(claims.exp, Some((now + Duration::days(60)).timestamp() as usize));
        assert!(matches!(claims.r#type, TokenType::UserToken));
        assert_eq!(view.expiry_date, now + Duration::days(60));
        assert!(view.admin);
    }

    #[tokio::test]
    async fn login_returns_unauthorized_when_password_wrong() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::user_model_with_password(1, fixtures::PASSWORD)]])
            .into_connection();
        let login_request = UserLogin { username: "tester".into(), password: "Wrong1234".into() };

        let result = login(&login_request, &"test-secret".to_string(), &db, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert!(matches!(result, Err(SrvErr::Unauthorized)));
    }

    #[tokio::test]
    async fn login_returns_unauthorized_when_user_unknown() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([no_users()]).into_connection();
        let login_request = UserLogin { username: "nobody".into(), password: fixtures::PASSWORD.into() };

        let result = login(&login_request, &"test-secret".to_string(), &db, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert!(matches!(result, Err(SrvErr::Unauthorized)));
    }

    #[tokio::test]
    async fn login_returns_rule_violation_when_username_empty() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let login_request = UserLogin { username: String::new(), password: fixtures::PASSWORD.into() };

        let result = login(&login_request, &"test-secret".to_string(), &db, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::LoginUsernameEmpty));
    }

    #[tokio::test]
    async fn login_returns_rule_violation_when_password_empty() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let login_request = UserLogin { username: "tester".into(), password: String::new() };

        let result = login(&login_request, &"test-secret".to_string(), &db, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::LoginPasswordEmpty));
    }

    fn password_change(old: &str, new: &str, repeat: &str) -> PasswordChange {
        PasswordChange { old_password: old.into(), new_password: new.into(), password_repeat: repeat.into() }
    }

    #[tokio::test]
    async fn change_password_rehashes_with_fixed_salt() {
        test_support::init();
        let stored = fixtures::user_model_with_password(1, fixtures::PASSWORD);
        let db = test_support::db::mock()
            .append_query_results([vec![stored.clone()]])
            .append_query_results([vec![stored]])
            .into_connection();

        let result = change_password(password_change(fixtures::PASSWORD, "NewPassw0rd", "NewPassw0rd"), &fixtures::user(), &db, &SeqRandom::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        let hash = string_value_written(&log, "UPDATE \"users\"", "$argon2").expect("a new password hash is stored");
        assert!(hash.contains(SALT), "hash {hash} does not use the fixed salt");
        assert_eq!(hash, fixtures::password_hash("NewPassw0rd"));
    }

    #[tokio::test]
    async fn change_password_returns_unauthorized_when_old_password_wrong() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::user_model_with_password(1, fixtures::PASSWORD)]])
            .into_connection();

        let result = change_password(password_change("Wrong1234", "NewPassw0rd", "NewPassw0rd"), &fixtures::user(), &db, &SeqRandom::new()).await;

        assert!(matches!(result, Err(SrvErr::Unauthorized)));
        assert_no_statement_starting_with(&test_support::db::log(db), "UPDATE");
    }

    #[tokio::test]
    async fn change_password_returns_rule_violation_when_new_passwords_do_not_match() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::user_model_with_password(1, fixtures::PASSWORD)]])
            .into_connection();

        let result = change_password(password_change(fixtures::PASSWORD, "NewPassw0rd", "Other1234"), &fixtures::user(), &db, &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::PasswordChangePasswordsDoNotMatch));
        assert_no_statement_starting_with(&test_support::db::log(db), "UPDATE");
    }

    #[tokio::test]
    async fn delete_removes_user() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::user_model(2)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = delete(Uuid::from_u128(2), &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, "DELETE FROM \"users\"", &[Value::from(2)]);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_user_missing() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([no_users()]).into_connection();

        let result = delete(Uuid::from_u128(2), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
    }
}
