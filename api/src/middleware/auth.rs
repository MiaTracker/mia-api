use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Request, StatusCode};
use axum::middleware::Next;
use axum::response::IntoResponse;
use jsonwebtoken::{decode, DecodingKey, Validation};
use services::infrastructure::SrvErr;
use views::users::UserTokenClaims;
use crate::infrastructure::{ApiErr, ApiErrView, AppState, ErrorKey};

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

    let claims = match decode::<UserTokenClaims>(&token, &DecodingKey::from_secret(state.jwt_secret.as_ref()), &Validation::default()) {
        Ok(data) => { data }
        Err(_) => {
           return Err(ApiErr {
               errors: vec![ApiErrView { key: ErrorKey::InvalidAuthenticationToken.to_string(), debug_message: "Invalid authentication token!".to_string() }],
               status: StatusCode::UNAUTHORIZED,
           })
       }
    }.claims;

    let user_id = match uuid::Uuid::parse_str(&claims.sub) {
        Ok(id) => { id }
        Err(_) => {
            return Err(ApiErr {
                errors: vec![ApiErrView { key: ErrorKey::InvalidAuthenticationToken.to_string(), debug_message: "Invalid authentication token!".to_string() }],
                status: StatusCode::UNAUTHORIZED
            });
        }
    };

    let user = match services::users::query_user_by_uuid(user_id, &state.conn).await {
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
            return Err(<SrvErr as Into<ApiErr>>::into(err))
        }
    };

    req.extensions_mut().insert(user);
    Ok(next.run(req).await)
}