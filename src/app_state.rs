use std::sync::Arc;

use anyhow::Result;
use sqlx::PgPool;

use crate::services::{
    AuthService, AuthServiceTrait, CloudStorageService, GcsService, GoogleAuthService,
    GoogleAuthServiceTrait, MetricsService, MetricsServiceTrait, VideoProcessingService,
    VideoProcessingServiceTrait, VideoService, VideoServiceTrait,
};

#[derive(Clone)]
pub struct AppState {
    pub video_service: Arc<dyn VideoServiceTrait>,
    pub storage_service: Arc<dyn CloudStorageService>,
    pub video_processing_service: Arc<dyn VideoProcessingServiceTrait>,
    pub auth_service: Arc<dyn AuthServiceTrait>,
    pub google_auth_service: Arc<dyn GoogleAuthServiceTrait>,
    pub metrics_service: Arc<dyn MetricsServiceTrait>,
}

impl AppState {
    pub async fn initialize(pool: PgPool) -> Result<Self> {
        let jwt_secret =
            std::env::var("JWT_SECRET").map_err(|_| anyhow::anyhow!("JWT_SECRET must be set"))?;

        let video_service: Arc<dyn VideoServiceTrait> = Arc::new(VideoService::new(pool.clone()));

        let metrics_service: Arc<dyn MetricsServiceTrait> = MetricsService::new(pool.clone());

        let storage_service: Arc<dyn CloudStorageService> = Arc::new(GcsService::new().await?);

        let auth_service: Arc<dyn AuthServiceTrait> =
            Arc::new(AuthService::new(pool.clone(), jwt_secret.clone()));

        let google_auth_service: Arc<dyn GoogleAuthServiceTrait> =
            Arc::new(GoogleAuthService::new(pool.clone(), jwt_secret.clone()));

        let video_processing_service: Arc<dyn VideoProcessingServiceTrait> =
            Arc::new(VideoProcessingService::new(
                Arc::clone(&video_service),
                Arc::clone(&storage_service),
                Arc::clone(&metrics_service),
            ));

        Ok(Self {
            video_service,
            storage_service,
            video_processing_service,
            auth_service,
            google_auth_service,
            metrics_service,
        })
    }
}
