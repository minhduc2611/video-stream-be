use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub validation_errors: Option<HashMap<String, Vec<String>>>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            validation_errors: None,
        }
    }

    pub fn error(message: &str, validation_errors: Option<HashMap<String, Vec<String>>>) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(message.to_string()),
            validation_errors,
        }
    }
}
