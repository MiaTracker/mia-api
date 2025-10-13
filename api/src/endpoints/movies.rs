use crate::infrastructure::{AppState, IntoApiResponse};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Extension, Json};
use views::api::{MaybeRouteType, RouteType};
use views::images::{BackdropUpdate, ImagesUpdate, PosterUpdate, Images};
use views::media::{MediaCreateParams, MediaDeletePathParams, MediaSourceCreate, MediaSourceDelete, MediaType, PageReq, SearchParams, SearchQuery, MediaIndex, SearchResults};
use views::movies::{MovieDetails, MovieMetadata};
use views::users::CurrentUser;
use views::api::ApiErrView;

#[utoipa::path(
    post,
    path = "/movies",
    params(MediaCreateParams),
    responses(
        (status = 200, description = "Movie already exists", body = i32),
        (status = 201, description = "Movie created", body = i32),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::movies::create(params.tmdb_id, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

#[utoipa::path(
    post,
    path = "/movies/source_create",
    request_body = MediaSourceCreate,
    responses(
        (status = 200, description = "Movie already exists", body = i32),
        (status = 201, description = "Movie created", body = i32),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceCreate>) -> impl IntoResponse {
    let result = services::media::create_w_source(json, MediaType::Movie, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

#[utoipa::path(
    get,
    path = "/movies",
    params(PageReq),
    responses(
        (status = 200, description = "All movie indexes", body = [MediaIndex]),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::movies::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/movies/search",
    params(SearchParams),
    request_body = SearchQuery,
    responses(
        (status = 200, description = "Movie indexes matching the search criteria", body = SearchResults),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Query(params): Query<SearchParams>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::media::search(search, params.committed, params.into(), MaybeRouteType::Movies.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/movies/genres",
    responses(
        (status = 200, description = "All genres of user's movies", body = [String]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn genres(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::genres::index(Some(MediaType::Movie), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/movies/{movie_id}",
    params(
        ("movie_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "Movie details", body = MovieDetails),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::details(movie_id, &user, &state.conn).await;
    result.map_to_response(|movie: &Option<MovieDetails>| {
        match movie {
            None => { (StatusCode::NOT_FOUND, None) }
            Some(details) => { (StatusCode::OK, Some(details)) }
        }
    })
}

#[utoipa::path(
    get,
    path = "/movies/{movie_id}/metadata",
    params(
        ("movie_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "Movie metadata", body = MovieMetadata),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn metadata(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::metadata(movie_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/movies/{movie_id}/on_watchlist",
    params(
        ("movie_id" = i32, Path, )
    ),
    responses(
    (status = 200, description = "Weather the movie is currently on watchlist", body = bool),
    (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
    (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
    (status = 404, description = "The movie was not found"),
    (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn on_watchlist(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::on_watchlist(movie_id, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    path = "/movies/{movie_id}/metadata",
    params(
        ("movie_id" = i32, Path, )
    ),
    request_body = MovieMetadata,
    responses(
        (status = 200, description = "Movie metadata updated"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>,
                    Json(metadata): Json<MovieMetadata>) -> impl IntoResponse {
    let result = services::movies::update(movie_id, metadata, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/movies/{movie_id}/backdrops",
    params(
        ("movie_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "All backdrops of the movie", body = Images),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn backdrops(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::backdrops(movie_id, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    path = "/movies/{movie_id}/backdrops/default",
    params(
        ("movie_id" = i32, Path, )
    ),
    request_body = ImagesUpdate,
    responses(
        (status = 200, description = "Default movie backdrop changed"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update_backdrop(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                           Path(movie_id): Path<i32>, Json(json): Json<BackdropUpdate>) -> impl IntoResponse {
    let result = services::media::update_backdrop(movie_id, MediaType::Movie, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/movies/{movie_id}/posters",
    params(
        ("movie_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "All posters of the movie", body = Images),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn posters(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::posters(movie_id, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    path = "/movies/{movie_id}/posters/default",
    params(
        ("movie_id" = i32, Path, )
    ),
    request_body = ImagesUpdate,
    responses(
        (status = 200, description = "Default movie poster changed"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update_poster(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(movie_id): Path<i32>, Json(json): Json<PosterUpdate>) -> impl IntoResponse {
    let result = services::media::update_poster(movie_id, MediaType::Movie, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/movies/{movie_id}/images",
    params(
        ("movie_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "All images of the movie", body = Images),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn images(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::images(movie_id, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    path = "/movies/{movie_id}/images",
    params(
        ("movie_id" = i32, Path, )
    ),
    request_body = ImagesUpdate,
    responses(
        (status = 200, description = "Default movie images changed"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update_images(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                           Path(movie_id): Path<i32>, Json(json): Json<ImagesUpdate>) -> impl IntoResponse {
    let result = services::media::update_images(movie_id, MediaType::Movie, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    delete,
    path = "/movies/{movie_id}",
    params(MediaDeletePathParams),
    responses(
        (status = 200, description = "Movie deleted"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(path): Path<MediaDeletePathParams>) -> impl IntoResponse {
    let result = services::media::delete(path.media_id, RouteType::Movies.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/movies/source_delete",
    request_body = MediaSourceDelete,
    responses(
        (status = 200, description = "Movie deleted"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The movie was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn delete_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceDelete>) -> impl IntoResponse {
    let result = services::media::delete_w_source(json.tmdb_id, json.source, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}