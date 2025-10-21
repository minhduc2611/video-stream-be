use actix_cors::Cors;
use actix_web::{web, App, HttpServer, middleware::Logger};
use dotenv::dotenv;
use std::env;

mod handlers;
mod middleware;
mod models;
mod services;
mod utils;

use handlers::{auth, videos};
use middleware::auth_middleware;
use services::database;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    env_logger::init();

    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");
    
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid number");

    // Initialize database connection pool
    let pool = database::create_pool(&database_url).await
        .expect("Failed to create database pool");

    log::info!("Starting server on port {}", port);

    HttpServer::new(move || {
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(cors)
            .wrap(Logger::default())
            .service(
                web::scope("/api/v1")
                    .service(
                        web::scope("/auth")
                            .route("/register", web::post().to(auth::register))
                            .route("/login", web::post().to(auth::login))
                            .route("/google", web::post().to(auth::google_auth))
                            .route("/logout", web::post().to(auth::logout))
                            .route("/me", web::get().to(auth::me).wrap(auth_middleware::AuthMiddleware))
                    )
                    .service(
                        web::scope("/videos")
                            .wrap(auth_middleware::AuthMiddleware)
                            // .route("", web::get().to(videos::list_videos))
                            .route("", web::post().to(videos::upload_video))
                            // .route("/{id}", web::get().to(videos::get_video))
                            // .route("/{id}/stream", web::get().to(videos::stream_video))
                            // .route("/{id}/stream/{filename}", web::get().to(videos::serve_hls_file))
                            // .route("/{id}/thumbnail", web::get().to(videos::get_thumbnail))
                            // .route("/{id}", web::delete().to(videos::delete_video))
                    )
                    // .route("/health", web::get().to(health::health_check))
            )
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await
}