use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use services::infrastructure::SrvErr;

pub trait IntoApiResponse<'b, R: 'b> {
    fn to_response(&'b self, success: StatusCode) -> Response
        where R: Serialize;
    fn map_to_response<F, U>(&'b self, f: F) -> Response
        where F: Fn(&'b R) -> (StatusCode, Option<&'b U>), U: Serialize + 'b;
    fn map_to_status<F>(&'b self, f: F) -> Response
        where F: for<'a> Fn(&'a R) -> StatusCode, R: Serialize;
    fn map_to_status_and_result<F, T>(&'b self, f: F) -> Response
        where F: for<'a> Fn(&'a R) -> (StatusCode, T), R: Serialize, T: Serialize;
}

impl<'b, R: 'b> IntoApiResponse<'b, R> for Result<R, SrvErr> {
    fn to_response(&'b self, success: StatusCode) -> Response
        where R: Serialize {
        match self {
            Ok(data) => { (success, axum::Json(data)).into_response() }
            Err(err) => {
                let api_err: views::api::ApiErr = err.into();
                api_err.into_response()
            }
        }
    }

    fn map_to_response<F, U>(&'b self, f: F) -> Response
        where F: Fn(&'b R) -> (StatusCode, Option<&'b U>), U: Serialize + 'b {

        match self {
            Ok(data) => {
                let res = f(&data);
                if let Some(r) = res.1 {
                    (res.0, Json(r)).into_response()
                } else {
                    res.0.into_response()
                }
            }
            Err(err) => {
                let api_err: views::api::ApiErr = err.into();
                api_err.into_response()
            }
        }
    }

    fn map_to_status<F>(&'b self, f: F) -> Response
        where F: for<'a> Fn(&'a R) -> StatusCode, R: Serialize {
        match self {
            Ok(data) => { f(&data).into_response() }
            Err(err) => {
                let api_err: views::api::ApiErr = err.into();
                api_err.into_response()
            }
        }
    }

    fn map_to_status_and_result<F, T>(&'b self, f: F) -> Response
        where F: for<'a> Fn(&'a R) -> (StatusCode, T), R: Serialize, T: Serialize {
        match self {
            Ok(data) => { let data = f(&data); (data.0, axum::Json(data.1)).into_response() }
            Err(err) => {
                let api_err: views::api::ApiErr = err.into();
                api_err.into_response()
            }
        }
    }
}