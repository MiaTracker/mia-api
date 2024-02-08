use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::IntoResponse;
use views::api::{ApiErr, ApiErrView, ErrorKey};
use views::users::CurrentUser;

pub async fn user(req: Request<Body>, next: Next) -> Result<impl IntoResponse, ApiErr> {
    let user = req.extensions().get::<CurrentUser>().expect("CurrentUser not set! Middleware order is incorrect!");
    if user.though_bot {
        return Err(ApiErr {
            errors: vec![ApiErrView { key: ErrorKey::InsufficientPermissions.to_string(), debug_message: "Bots are not allowed to access this endpoint!".to_string() }],
            status: StatusCode::UNAUTHORIZED
        });
    }

    Ok(next.run(req).await)
}