use std::sync::Arc;

use actix_web::{web, HttpResponse, Result};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::app_state::AppState;
use crate::utils::response::ApiResponse;

#[derive(Debug, Deserialize)]
pub struct PlaybackMetricPayload {
    pub benchmark_run_source: Option<String>,
    pub benchmark_metadata: Option<Value>,
    pub video_id: Option<Uuid>,
    pub country: Option<String>,
    pub isp: Option<String>,
    pub device_type: Option<String>,
    pub bandwidth_mbps: Option<f64>,
    pub first_frame_ms: Option<i64>,
    pub total_startup_ms: Option<i64>,
    pub buffering_events: Option<i32>,
}

pub async fn record_playback_metric(
    app_state: web::Data<AppState>,
    payload: web::Json<PlaybackMetricPayload>,
) -> Result<HttpResponse> {
    let metrics_service = Arc::clone(&app_state.metrics_service);
    let payload = payload.into_inner();

    let mut benchmark_run_id = None;

    let mut notes_map = match payload.benchmark_metadata.clone() {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = Map::new();
            map.insert("payload".to_string(), other);
            map
        }
        None => Map::new(),
    };

    if let Some(video_id) = payload.video_id {
        notes_map
            .entry("video_id".to_string())
            .or_insert(json!(video_id));
    }
    if let Some(bandwidth) = payload.bandwidth_mbps {
        notes_map.insert("bandwidth_mbps".to_string(), json!(bandwidth));
    }
    if let Some(country) = payload.country.as_ref() {
        notes_map
            .entry("country".to_string())
            .or_insert(json!(country));
    }
    if let Some(isp) = payload.isp.as_ref() {
        notes_map
            .entry("isp".to_string())
            .or_insert(json!(isp));
    }
    if let Some(device) = payload.device_type.as_ref() {
        notes_map
            .entry("device_type".to_string())
            .or_insert(json!(device));
    }

    let benchmark_notes = if notes_map.is_empty() {
        None
    } else {
        Some(Value::Object(notes_map))
    };

    if let Some(source) = payload.benchmark_run_source.as_deref() {
        match metrics_service
            .create_benchmark_run(source, benchmark_notes.clone())
            .await
        {
            Ok(id) => benchmark_run_id = Some(id),
            Err(err) => {
                log::error!(
                    "Failed to create benchmark run for playback metric: {}",
                    err
                );
                return Ok(
                    HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                        "Failed to initialize benchmark run",
                        None,
                    )),
                );
            }
        }
    }

    if let Err(err) = metrics_service
        .record_playback_metric(
            benchmark_run_id,
            payload.country.as_deref(),
            payload.isp.as_deref(),
            payload.device_type.as_deref(),
            payload.first_frame_ms,
            payload.total_startup_ms,
            payload.buffering_events,
        )
        .await
    {
        log::error!("Failed to record playback metric: {}", err);
        return Ok(
            HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Failed to record playback metric",
                None,
            )),
        );
    }

    Ok(HttpResponse::Accepted().json(ApiResponse::success("metric recorded")))
}

pub async fn get_metrics_insights(app_state: web::Data<AppState>) -> Result<HttpResponse> {
    let metrics_service = Arc::clone(&app_state.metrics_service);

    match metrics_service.fetch_insights().await {
        Ok(insights) => Ok(HttpResponse::Ok().json(ApiResponse::success(insights))),
        Err(err) => {
            log::error!("Failed to load metrics insights: {}", err);
            Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                    "Failed to load metrics insights",
                    None,
                )),
            )
        }
    }
}
