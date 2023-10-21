#[derive(Debug)]
pub struct Error {
    pub message: String
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self {
            message: value.to_string()
        }
    }
}

#[macro_export]
macro_rules! assert_request {
    ($response:expr) => { if $response.status() != reqwest::StatusCode::OK { return Err(crate::infrastructure::Error { message: "Request to integration resource failed.".to_string() }) } };
}