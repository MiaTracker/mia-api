use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
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
use crate::infrastructure::{RuleViolation, SrvErr};
use crate::infrastructure::traits::IntoActiveModel;

static PASS_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^(?=.*[a-z])(?=.*[A-Z])(?=.*\d)[a-zA-Z\d\w\W]{7,}$"#).expect("Invalid password regex!")
});

pub async fn register(user: &UserRegistration, db: &DbConn) -> Result<(), SrvErr> {
    let email_regex = Regex::new(r#"(?:[a-z0-9!#$%&'*+/=?^_`{|}~-]+(?:\.[a-z0-9!#$%&'*+/=?^_`{|}~-]+)*|"(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21\x23-\x5b\x5d-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])*")@(?:(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.)+[a-z0-9](?:[a-z0-9-]*[a-z0-9])?|\[(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9])\.){3}(?:(2(5[0-5]|[0-4][0-9])|1[0-9][0-9]|[1-9]?[0-9])|[a-z0-9-]*[a-z0-9]:(?:[\x01-\x08\x0b\x0c\x0e-\x1f\x21-\x5a\x53-\x7f]|\\[\x01-\x09\x0b\x0c\x0e-\x7f])+)])"#).expect("Regex is invalid!");

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

    let salt = SaltString::generate(&mut OsRng);
    let hashed_password = match Argon2::default().hash_password(user.password.as_bytes(), &salt) {
        Ok(hash) => { hash }
        Err(err) => {
            return Err(SrvErr::Internal(format!("Error while hashing password: {}", err)));
        }
    }.to_string();

    let mut model = user.into_active_model();
    model.password_hash = Set(hashed_password);
    match model.insert(db).await {
        Ok(_) => { Ok(()) }
        Err(db_err) => { Err(SrvErr::DB(db_err)) }
    }
}

pub async fn login(user: &UserLogin, jwt_secret: &String, db: &DbConn) -> Result<UserToken, SrvErr> {
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

    let now = chrono::Utc::now();
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

pub async fn change_password(pass_change: PasswordChange, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
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

    let salt = SaltString::generate(&mut OsRng);
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