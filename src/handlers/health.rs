use crate::utils::response::ApiResponse;
use actix_web::{HttpResponse, Result};

pub async fn health_check() -> Result<HttpResponse> {
    Ok(
        HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now()
        }))),
    )
}
