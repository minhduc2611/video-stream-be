use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse, Result};
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

#[derive(Debug)]
struct GeoLookupResult {
    country: Option<String>,
    region: Option<String>,
    city: Option<String>,
    latitude: Option<String>,
    longitude: Option<String>,
    timezone: Option<String>,
}

fn normalize_geo_field(value: String) -> Option<String> {
    let trimmed = value.trim().trim_matches('"').trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null") {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_ip(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(addr) = trimmed.parse::<SocketAddr>() {
        return Some(addr.ip().to_string());
    }

    if let Ok(addr) = trimmed.parse::<IpAddr>() {
        return Some(addr.to_string());
    }

    if let Some(stripped) = trimmed
        .strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
    {
        if let Ok(addr) = stripped.parse::<IpAddr>() {
            return Some(addr.to_string());
        }
    }

    None
}

fn extract_client_ip(req: &HttpRequest) -> Option<String> {
    if let Some(real_ip) = req.connection_info().realip_remote_addr() {
        if let Some(ip) = normalize_ip(real_ip) {
            return Some(ip);
        }
    }

    if let Some(forwarded_for) = req
        .headers()
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
    {
        if let Some(first) = forwarded_for.split(',').next() {
            if let Some(ip) = normalize_ip(first) {
                return Some(ip);
            }
        }
    }

    req.peer_addr().map(|addr| addr.ip().to_string())
}

fn is_unknown_country(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("unknown")
        || trimmed.eq_ignore_ascii_case("null")
}

pub async fn record_playback_metric(
    req: HttpRequest,
    app_state: web::Data<AppState>,
    payload: web::Json<PlaybackMetricPayload>,
) -> Result<HttpResponse> {
    let metrics_service = Arc::clone(&app_state.metrics_service);
    let mut payload = payload.into_inner();
    let client_ip = extract_client_ip(&req);
    let mut derived_geo: Option<GeoLookupResult> = None;
    let needs_country = payload
        .country
        .as_ref()
        .map(|value| is_unknown_country(value))
        .unwrap_or(true);

    println!("<><><><><><><> needs_country: {}", needs_country);
    if needs_country {
        if let Some(ip) = client_ip.as_deref() {
            let ip_string = ip.to_string();
            match tokio::task::spawn_blocking({
                println!("<><><><><><><>  ip_string: {}", ip_string);
                let ip_clone = ip_string.clone();
                move || geolocation::find(&ip_clone)
            })
            .await
            {
                Ok(Ok(locator)) => {
                    println!("<><><><><><><>  country: {}", locator.country);
                    println!("<><><><><><><>  region: {}", locator.region);
                    println!("<><><><><><><>  city: {}", locator.city);
                    println!("<><><><><><><>  latitude: {}", locator.latitude);
                    println!("<><><><><><><>  longitude: {}", locator.longitude);
                    println!("<><><><><><><>  timezone: {}", locator.timezone);
                    let geo = GeoLookupResult {
                        country: normalize_geo_field(locator.country),
                        region: normalize_geo_field(locator.region),
                        city: normalize_geo_field(locator.city),
                        latitude: normalize_geo_field(locator.latitude),
                        longitude: normalize_geo_field(locator.longitude),
                        timezone: normalize_geo_field(locator.timezone),
                    };

                    if let Some(country) = geo.country.as_ref() {
                        payload.country = Some(country.clone());
                    }

                    derived_geo = Some(geo);
                }
                Ok(Err(err)) => {
                    log::warn!("Geolocation lookup failed for {}: {}", ip_string, err);
                }
                Err(err) => {
                    log::warn!("Failed to join geolocation task for {}: {}", ip_string, err);
                }
            }
        }
    }

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
        notes_map.entry("isp".to_string()).or_insert(json!(isp));
    }
    if let Some(device) = payload.device_type.as_ref() {
        notes_map
            .entry("device_type".to_string())
            .or_insert(json!(device));
    }
    if let Some(ip) = client_ip.as_ref() {
        notes_map
            .entry("client_ip".to_string())
            .or_insert(json!(ip));
    }
    if let Some(geo) = derived_geo.as_ref() {
        let mut geo_map = Map::new();
        if let Some(country) = geo.country.as_ref() {
            geo_map.insert("country".to_string(), json!(country));
        }
        if let Some(region) = geo.region.as_ref() {
            geo_map.insert("region".to_string(), json!(region));
        }
        if let Some(city) = geo.city.as_ref() {
            geo_map.insert("city".to_string(), json!(city));
        }
        if let Some(latitude) = geo.latitude.as_ref() {
            geo_map.insert("latitude".to_string(), json!(latitude));
        }
        if let Some(longitude) = geo.longitude.as_ref() {
            geo_map.insert("longitude".to_string(), json!(longitude));
        }
        if let Some(timezone) = geo.timezone.as_ref() {
            geo_map.insert("timezone".to_string(), json!(timezone));
        }

        if !geo_map.is_empty() {
            notes_map.insert("geo_lookup".to_string(), Value::Object(geo_map));
        }
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
