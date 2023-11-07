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

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self {
            message: value.to_string()
        }
    }
}

#[macro_export]
macro_rules! assert_request {
    ($response:expr) => { if $response.status() != reqwest::StatusCode::OK {
        return Err(crate::infrastructure::Error { message: "Request to integration resource failed.".to_string() })
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