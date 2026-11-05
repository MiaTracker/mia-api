use std::convert::Infallible;
use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::stream::{Stream, StreamExt};

use services::chat::ExchangeSubscription;
use views::api::ApiErrView;
use views::conversations::{Conversation, ConversationDetails, ConversationParams, ExchangeCreate, ExchangeEvent, ExchangeStreamParams};
use views::media::PageReq;
use views::users::CurrentUser;

use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    operation_id = "conversations::create",
    path = "/conversations",
    request_body = ExchangeCreate,
    responses(
        (status = 200, description = "Server-sent events of the new conversation's first exchange: conversation_created, then thinking_delta, text_delta and tool_call, then exchange_completed or exchange_failed, then title if one was generated, and finally stream_end. Every stream ends with stream_end", body = ExchangeEvent, content_type = "text/event-stream"),
        (status = 400, description = "The request is invalid, or the message content is empty (ExchangeContentEmpty)", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(body): Json<ExchangeCreate>) -> Response {
    let result = services::chat::create(body, &user, &state.conn, state.registry.as_ref(), state.jobs.as_ref(), state.clock.as_ref()).await;
    match result {
        Ok(subscription) => sse(subscription).into_response(),
        Err(err) => Err::<(), _>(err).to_response(StatusCode::OK)
    }
}

#[utoipa::path(
    get,
    operation_id = "conversations::index",
    path = "/conversations",
    params(PageReq),
    responses(
        (status = 200, description = "The user's conversations", body = [Conversation]),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::chat::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "conversations::details",
    path = "/conversations/{conversation_id}",
    params(ConversationParams),
    responses(
        (status = 200, description = "Conversation details", body = ConversationDetails),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The conversation was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<ConversationParams>) -> impl IntoResponse {
    let result = services::chat::details(params.conversation_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    delete,
    operation_id = "conversations::delete",
    path = "/conversations/{conversation_id}",
    params(ConversationParams),
    responses(
        (status = 200, description = "Conversation deleted"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The conversation was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<ConversationParams>) -> impl IntoResponse {
    let result = services::chat::delete(params.conversation_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    operation_id = "conversations::create_exchange",
    path = "/conversations/{conversation_id}/exchanges",
    params(ConversationParams),
    request_body = ExchangeCreate,
    responses(
        (status = 200, description = "Server-sent events of the new exchange: thinking_delta, text_delta and tool_call, then exchange_completed or exchange_failed, and finally stream_end. Every stream ends with stream_end", body = ExchangeEvent, content_type = "text/event-stream"),
        (status = 400, description = "The request is invalid, or the conversation already has an exchange in progress (ConversationExchangeInProgress)", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The conversation was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create_exchange(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Path(params): Path<ConversationParams>, Json(body): Json<ExchangeCreate>) -> Response {
    let result = services::chat::start_exchange(params.conversation_id, body, &user, &state.conn,
        state.registry.as_ref(), state.jobs.as_ref(), state.clock.as_ref()).await;
    match result {
        Ok(subscription) => sse(subscription).into_response(),
        Err(err) => Err::<(), _>(err).to_response(StatusCode::OK)
    }
}

#[utoipa::path(
    get,
    operation_id = "conversations::stream",
    path = "/conversations/{conversation_id}/exchanges/{exchange_id}/stream",
    params(ExchangeStreamParams),
    responses(
        (status = 200, description = "Server-sent events replaying the exchange so far, then following it live until it ends: thinking_delta, text_delta and tool_call, then exchange_completed or exchange_failed, and finally stream_end. Every stream ends with stream_end", body = ExchangeEvent, content_type = "text/event-stream"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The conversation or exchange was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn stream(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<ExchangeStreamParams>) -> Response {
    let result = services::chat::subscribe(params.conversation_id, params.exchange_id, &user, &state.conn, state.registry.as_ref()).await;
    match result {
        Ok(subscription) => sse(subscription).into_response(),
        Err(err) => Err::<(), _>(err).to_response(StatusCode::OK)
    }
}

#[utoipa::path(
    post,
    operation_id = "conversations::retry",
    path = "/conversations/{conversation_id}/exchanges/{exchange_id}/retry",
    params(ExchangeStreamParams),
    responses(
        (status = 200, description = "Server-sent events of the retried exchange, which moves to the end of the conversation: first the blocks it kept, then thinking_delta, text_delta and tool_call, then exchange_completed or exchange_failed, and finally stream_end. Every stream ends with stream_end", body = ExchangeEvent, content_type = "text/event-stream"),
        (status = 400, description = "The request is invalid, the exchange is not failed (ExchangeNotFailed), or the conversation already has an exchange in progress (ConversationExchangeInProgress)", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The conversation or exchange was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn retry(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<ExchangeStreamParams>) -> Response {
    let result = services::chat::retry_exchange(params.conversation_id, params.exchange_id, &user, &state.conn,
        state.registry.as_ref(), state.jobs.as_ref(), state.clock.as_ref()).await;
    match result {
        Ok(subscription) => sse(subscription).into_response(),
        Err(err) => Err::<(), _>(err).to_response(StatusCode::OK)
    }
}

/// Streams the subscription's events (see `ExchangeSubscription::into_events`), which end with `stream_end`.
fn sse(subscription: ExchangeSubscription) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = subscription.into_events().map(|event| {
        Ok(Event::default().event(event.name()).json_data(&event).unwrap_or_else(|_| Event::default().event(event.name())))
    });
    Sse::new(events).keep_alive(KeepAlive::default())
}
