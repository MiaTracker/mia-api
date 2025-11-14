use axum::body::Body;
use axum::extract::Path;
use axum::http::header;
use axum::response::IntoResponse;
use views::api::ApiErrView;

#[utoipa::path(
    get,
    operation_id = "images::get_local",
    path = "/img/{slug}/{name}",
    responses(
        (status = 200, description = "Image"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn get_local(Path((slug, name)): Path<(String, String)>) -> impl IntoResponse {
    let res = services::images::get_local(slug, name).await;
    match res {
        Ok(data) => {
            let header = [
                (header::CONTENT_TYPE, data.0)
            ];
            (header, Body::from_stream(data.1)).into_response()
        }
        Err(err) => {
            let api_err: views::api::ApiErr = (&err).into();
            api_err.into_response()
        }
    }
}


#[utoipa::path(
    get,
    operation_id = "images::get_tmdb",
    path = "/img/tmdb/{slug}/{path}",
    responses(
        (status = 200, description = "Image"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn get_tmdb(Path((slug, path)): Path<(String, String)>) -> impl IntoResponse {
    let res = services::images::get_tmdb(slug, path).await;
    match res {
        Ok(data) => {
            let header = [
                (header::CONTENT_TYPE, data.1)
            ];
            (data.0, header, Body::from_stream(data.2)).into_response()
        }
        Err(err) => {
            let api_err: views::api::ApiErr = (&err).into();
            api_err.into_response()
        }
    }
}