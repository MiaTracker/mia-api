use jsonwebtoken::{encode, EncodingKey, Header};
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder};
use entities::app_tokens;
use entities::prelude::AppTokens;
use views::app_tokens::{AppToken, AppTokenIndex};
use views::users::{CurrentUser, TokenClaims, TokenType};
use crate::infrastructure::{Clock, Random, RuleViolation, SrvErr};

pub async fn generate(name: String, jwt_secret: &String, user: &CurrentUser, db: &DbConn, clock: &dyn Clock, random: &dyn Random) -> Result<AppToken, SrvErr> {
    if name.is_empty() {
        return Err(SrvErr::RuleViolation(vec![RuleViolation::AppTokenNameEmpty]))
    }

    let existing = AppTokens::find().filter(app_tokens::Column::UserId.eq(user.id))
        .filter(app_tokens::Column::Name.eq(&name)).count(db).await? != 0;

    if existing {
        return Err(SrvErr::RuleViolation(vec![RuleViolation::AppTokenNameAlreadyExists]));
    }

    let uuid = random.uuid();

    let claims = TokenClaims {
        sub: uuid,
        exp: None,
        iat: None,
        r#type: TokenType::AppToken
    };

    let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(jwt_secret.as_ref()))
        .map_err(|err| SrvErr::Internal(format!("Failed to encode token: {}", err)))?;

    let generated = clock.now();

    let model = app_tokens::ActiveModel {
        uuid: Set(uuid),
        name: Set(name.clone()),
        user_id: Set(user.id),
        generated: Set(generated.naive_utc())
    };
    model.insert(db).await?;

    Ok(AppToken {
        name,
        token,
        generated
    })
}

pub async fn index(user: &CurrentUser, db: &DbConn) -> Result<Vec<AppTokenIndex>, SrvErr> {
    let tokens = AppTokens::find().filter(app_tokens::Column::UserId.eq(user.id))
        .order_by_asc(app_tokens::Column::Name).all(db).await?.iter().map(|t | {
        AppTokenIndex::from(t)
    }).collect();
    Ok(tokens)
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

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{decode, DecodingKey, Validation};
    use sea_orm::Value;
    use uuid::Uuid;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_wrote, deleted};
    use crate::test_support::fixtures;
    use crate::test_support::{assert_single_violation, fixed_clock, SeqRandom};

    #[tokio::test]
    async fn generate_returns_token_with_uuid_from_random_and_generated_at_now() {
        test_support::init();
        let clock = fixed_clock(2026, 1, 1, 12, 30, 0);
        let user = fixtures::user();
        let uuid = Uuid::from_u128(1000);
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([vec![fixtures::app_token_model(uuid, "laptop", user.id)]])
            .into_connection();

        let token = generate("laptop".to_string(), &"test-secret".to_string(), &user, &db, &clock, &SeqRandom::new())
            .await.expect("generate should succeed");

        assert_eq!(token.name, "laptop");
        assert_eq!(token.generated, clock.now());
        let mut validation = Validation::default();
        validation.required_spec_claims.clear();
        validation.validate_exp = false;
        let claims = decode::<TokenClaims>(&token.token, &DecodingKey::from_secret(b"test-secret"), &validation)
            .expect("token decodes with the secret").claims;
        assert_eq!(claims.sub, uuid);
        assert!(matches!(claims.r#type, TokenType::AppToken));
        assert!(claims.exp.is_none() && claims.iat.is_none());
        let log = test_support::db::log(db);
        assert_wrote(&log, "INSERT INTO \"app_tokens\"", &[Value::from(uuid), Value::from("laptop"), Value::from(user.id), Value::from(clock.now().naive_utc())]);
    }

    #[tokio::test]
    async fn generate_returns_rule_violation_when_name_exists() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();

        let result = generate("laptop".to_string(), &"test-secret".to_string(), &fixtures::user(), &db, &fixed_clock(2026, 1, 1, 12, 30, 0), &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::AppTokenNameAlreadyExists));
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn generate_returns_rule_violation_when_name_empty() {
        test_support::init();
        let db = test_support::db::mock().into_connection();

        let result = generate(String::new(), &"test-secret".to_string(), &fixtures::user(), &db, &fixed_clock(2026, 1, 1, 12, 30, 0), &SeqRandom::new()).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::AppTokenNameEmpty));
    }

    #[tokio::test]
    async fn index_lists_tokens() {
        test_support::init();
        let user = fixtures::user();
        let db = test_support::db::mock()
            .append_query_results([vec![
                fixtures::app_token_model(Uuid::from_u128(1), "laptop", user.id),
                fixtures::app_token_model(Uuid::from_u128(2), "phone", user.id),
            ]])
            .into_connection();

        let tokens = index(&user, &db).await.expect("index should succeed");

        let names: Vec<&str> = tokens.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["laptop", "phone"]);
    }

    #[tokio::test]
    async fn revoke_deletes_token_by_name() {
        test_support::init();
        let user = fixtures::user();
        let uuid = Uuid::from_u128(7);
        let db = test_support::db::mock()
            .append_query_results([vec![fixtures::app_token_model(uuid, "laptop", user.id)]])
            .append_exec_results([deleted()])
            .into_connection();

        let result = revoke("laptop".to_string(), &user, &db).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, "DELETE FROM \"app_tokens\"", &[Value::from(uuid)]);
    }

    #[tokio::test]
    async fn revoke_returns_not_found_when_token_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::app_tokens::Model>::new()])
            .into_connection();

        let result = revoke("laptop".to_string(), &fixtures::user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
    }
}