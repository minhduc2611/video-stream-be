use actix_cors::Cors;
use actix_web::{middleware::Logger, web, App, HttpServer};
use dotenv::dotenv;
use std::env;
use std::time::Instant;

mod app_state;
mod handlers;
mod middleware;
mod models;
mod services;
mod utils;

use app_state::AppState;
use handlers::{auth, metrics, videos};
use middleware::{auth_middleware, MetricsMiddleware};
use serde_json::json;
use services::database;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Start measuring immediately at process start
    let cold_start_timer = Instant::now();

    dotenv().ok();
    env_logger::init();

    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid number");

    let pool = database::create_pool(&database_url)
        .await
        .expect("Failed to create database pool");

    let allowed_origins: Vec<String> = env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:3000".to_string())
        .split(',')
        .map(|origin| origin.trim().to_string())
        .filter(|origin| !origin.is_empty())
        .collect();

    let app_state = AppState::initialize(pool.clone())
        .await
        .expect("Failed to initialize application state");

    // Measure cold start before server bind
    let cold_start_duration_ms = cold_start_timer.elapsed().as_millis() as i64;

    log::info!("Cold start completed in {}ms", cold_start_duration_ms);

    let service_name = env::var("K_SERVICE").unwrap_or_else(|_| "video-stream-be".to_string());
    let revision = env::var("K_REVISION").ok();

    if let Err(err) = app_state
        .metrics_service
        .record_server_startup_metric(
            None,
            &service_name,
            revision.as_deref(),
            true,
            cold_start_duration_ms,
            Some(json!({
                "port": port,
            })),
        )
        .await
    {
        log::warn!("Failed to record server startup metric: {}", err);
    }

    HttpServer::new(move || {
        let cors = allowed_origins
            .iter()
            .fold(
                Cors::default()
                    .allow_any_method()
                    .allow_any_header()
                    .supports_credentials()
                    .max_age(3600),
                |cors, origin| cors.allowed_origin(origin),
            );

        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .wrap(cors)
            .wrap(Logger::default())
            .wrap(MetricsMiddleware {
                metrics_service: app_state.metrics_service.clone(),
            })
            .service(
                web::scope("/api/v1")
                    .service(
                        web::scope("/auth")
                            .route("/register", web::post().to(auth::register))
                            .route("/login", web::post().to(auth::login))
                            .route("/google", web::post().to(auth::google_auth))
                            .route("/logout", web::post().to(auth::logout))
                            .route(
                                "/me",
                                web::get()
                                    .to(auth::me)
                                    .wrap(auth_middleware::AuthMiddleware),
                            ),
                    )
                    .service(
                        web::scope("/videos")
                            .wrap(auth_middleware::AuthMiddleware)
                            .route("", web::get().to(videos::list_videos))
                            .route("", web::post().to(videos::upload_video))
                            .route("/{id}", web::get().to(videos::get_video))
                            .route("/{id}", web::put().to(videos::update_video))
                            .route("/{id}/stream", web::get().to(videos::stream_video))
                            .route("/{id}/thumbnail", web::get().to(videos::get_thumbnail))
                            .route("/{id}", web::delete().to(videos::delete_video)),
                    )
                    .service(
                        web::scope("/metrics")
                            .route("/playback", web::post().to(metrics::record_playback_metric))
                            .route("/insights", web::get().to(metrics::get_metrics_insights)),
                    ), // .route("/health", web::get().to(health::health_check))
            )
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await
}
