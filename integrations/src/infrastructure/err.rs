use std::fmt::{Display, Formatter};
use reqwest::StatusCode;

#[derive(Debug)]
pub struct Error {
    pub message: String,
    pub status_code: Option<StatusCode>,
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl From<reqwest::Error> for Error {
    fn from(value: reqwest::Error) -> Self {
        Self {
            message: value.to_string(),
            status_code: value.status(),
            source: Some(Box::new(value))
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self {
            message: value.to_string(),
            status_code: None,
            source: Some(Box::new(value)),
        }
    }
}

impl From<Error> for std::io::Error {
    fn from(value: Error) -> Self {
        value.source.map_or_else(
            || std::io::Error::from(std::io::ErrorKind::Other),
            |s| std::io::Error::new(std::io::ErrorKind::Other, s)
        )
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Integration error: {}{}", self.status_code
            .map_or("".to_string(), |s| format!("{} - ", s.as_str())),
            self.message)
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

#[macro_export]
macro_rules! assert_request {
    ($response:expr) => { if $response.status() != reqwest::StatusCode::OK {
        return Err(crate::infrastructure::Error { message: "Request to integration resource failed.".to_string(), status_code: Some($response.status()), source: None })
    }};
}

#[macro_export]
macro_rules! deserialize {
    ($typ:ty, $result:expr) => {
        {
            let status = $result.status();
            let text_rsp = $result.text().await;
            match text_rsp {
                Ok(text) => {
                    match serde_json::from_str::<$typ>(text.as_str()) {
                        Ok(val) => { val }
                        Err(err) => {
                            tracing::error!("Error deserializing integration response: {}", err.to_string());
                            tracing::error!("Server response: {}", status.to_string());
                            tracing::error!("{}", text);
                            return Err(err.into());
                        }
                    }
                }
                Err(err) => {
                    tracing::error!("Error reading integration response: {}", err.to_string());
                    tracing::error!("Server response: {}", status.to_string());
                    return Err(err.into());
                }
            }
        }
    };
}