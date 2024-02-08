use jsonwebtoken::{encode, EncodingKey, Header};
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, PaginatorTrait, QueryFilter};
use uuid::Uuid;
use entities::app_tokens;
use entities::prelude::AppTokens;
use views::app_tokens::AppToken;
use views::users::{CurrentUser, TokenClaims, TokenType};
use crate::infrastructure::{RuleViolation, SrvErr};

pub async fn generate(name: String, jwt_secret: &String, user: &CurrentUser, db: &DbConn) -> Result<AppToken, SrvErr> {
    let existing = AppTokens::find().filter(app_tokens::Column::UserId.eq(user.id))
        .filter(app_tokens::Column::Name.eq(&name)).count(db).await? != 0;

    if existing {
        return Err(SrvErr::RuleViolation(vec![RuleViolation::AppTokenNameAlreadyExists]));
    }

    let uuid = Uuid::new_v4();

    let claims = TokenClaims {
        sub: uuid,
        exp: None,
        iat: None,
        r#type: TokenType::AppToken
    };

    let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(jwt_secret.as_ref()))
        .map_err(|err| SrvErr::Internal(format!("Failed to encode token: {}", err)))?;

    let model = app_tokens::ActiveModel {
        uuid: Set(uuid),
        name: Set(name),
        user_id: Set(user.id)
    };
    model.insert(db).await?;

    Ok(AppToken { token })
}

pub async fn revoke(name: String, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let token = AppTokens::find().filter(app_tokens::Column::UserId.eq(user.id))
        .filter(app_tokens::Column::Name.eq(&name)).one(db).await?;

    if token.is_none() {
        return Err(SrvErr::NotFound);
    }
    let token = token.unwrap();

    token.delete(db).await?;

    Ok(())
}