use std::collections::HashSet;
use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::IntoResponse;
use jsonwebtoken::{decode, DecodingKey, Validation};
use once_cell::sync::Lazy;
use services::infrastructure::SrvErr;
use views::api::{ApiErr, ApiErrView, ErrorKey};
use views::users::{TokenClaims, TokenType};
use crate::infrastructure::AppState;

static JWT_VALIDATION: Lazy<Validation> = Lazy::new(|| {
    let mut val = Validation::default();
    val.validate_exp = false;
    val.required_spec_claims = HashSet::from(["sub".to_string(), "type".to_string()]);
    val
});


pub async fn auth(State(state): State<AppState>, mut req: Request<Body>, next: Next) -> Result<impl IntoResponse, ApiErr> {

    let token = req.headers().get(header::AUTHORIZATION)
        .and_then(|auth_header| auth_header.to_str().ok())
        .and_then(|auth_value| {
            if auth_value.starts_with("Bearer ") {
                Some(auth_value[7..].to_owned())
            } else {
                None
            }
        });

    let token = match token {
        None => {
            return Err(ApiErr {
                errors: vec![ApiErrView { key: ErrorKey::NoAuthenticationTokenProvided.to_string(), debug_message: "No authentication token provided, please login and provide received token.".to_string() }],
                status: StatusCode::UNAUTHORIZED,
            });
        }
        Some(x) => { x }
    };

    let claims = match decode::<TokenClaims>(&token, &DecodingKey::from_secret(state.jwt_secret.as_ref()), &JWT_VALIDATION) {
        Ok(data) => { data }
        Err(err) => {
            println!("{}", err);

           return Err(ApiErr {
               errors: vec![ApiErrView { key: ErrorKey::InvalidAuthenticationToken.to_string(), debug_message: "Invalid authentication token!".to_string() }],
               status: StatusCode::UNAUTHORIZED,
           })
       }
    }.claims;

    let res = match claims.r#type {
        TokenType::UserToken => {
            if let Some(exp) = claims.exp {
                if exp < chrono::Utc::now().timestamp() as usize {
                    return Err(ApiErr {
                        errors: vec![ApiErrView { key: ErrorKey::InvalidAuthenticationToken.to_string(), debug_message: "Authentication token expired!".to_string() }],
                        status: StatusCode::UNAUTHORIZED,
                    });
                }
            } else {
                return Err(ApiErr {
                    errors: vec![ApiErrView { key: ErrorKey::InvalidAuthenticationToken.to_string(), debug_message: "Invalid authentication token!".to_string() }],
                    status: StatusCode::UNAUTHORIZED,
                });
            }

            services::users::query_user_by_uuid(claims.sub, &state.conn).await
        }
        TokenType::AppToken => {
            services::users::query_user_by_app_token(claims.sub, &state.conn).await
        }
    };

    let user = match res {
        Ok(user) => {
            match user {
                None => {
                    return Err(ApiErr {
                        errors: vec![ApiErrView { key: ErrorKey::InvalidAuthenticationToken.to_string(), debug_message: "Invalid authentication token!".to_string() }],
                        status: StatusCode::UNAUTHORIZED
                    });
                }
                Some(user) => { user }
            }
        }
        Err(err) => {
            return Err(<&SrvErr as Into<ApiErr>>::into(&err))
        }
    };

    req.extensions_mut().insert(user);

    Ok(next.run(req).await)
}