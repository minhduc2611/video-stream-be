use crate::models::{CreateVideoRequest, PaginatedResponse, PaginationMeta, Video, VideoStatus};
use anyhow::Result;
use async_trait::async_trait;
use sqlx::PgPool;
use uuid::Uuid;

#[async_trait]
pub trait VideoServiceTrait: Send + Sync {
    async fn create_video(
        &self,
        request: CreateVideoRequest,
        user_id: Uuid,
        filename: String,
        original_filename: String,
        file_size: i64,
    ) -> Result<Video>;

    async fn get_video_by_id(&self, video_id: &Uuid) -> Result<Option<Video>>;

    async fn list_videos(
        &self,
        user_id: Option<Uuid>,
        limit: i64,
        offset: i64,
    ) -> Result<PaginatedResponse<Video>>;

    async fn update_video_status(&self, video_id: &Uuid, status: VideoStatus) -> Result<()>;

    async fn update_video_metadata(
        &self,
        video_id: &Uuid,
        duration: Option<i32>,
        thumbnail_path: Option<String>,
        hls_playlist_path: Option<String>,
    ) -> Result<()>;

    async fn delete_video(&self, video_id: &Uuid, user_id: &Uuid) -> Result<bool>;

    async fn update_video_details(
        &self,
        video_id: &Uuid,
        title: String,
        description: Option<String>,
    ) -> Result<Video>;
}

#[derive(Clone)]
pub struct VideoService {
    pool: PgPool,
}

impl VideoService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl VideoServiceTrait for VideoService {
    async fn create_video(
        &self,
        request: CreateVideoRequest,
        user_id: Uuid,
        filename: String,
        original_filename: String,
        file_size: i64,
    ) -> Result<Video> {
        let video_id = sqlx::query_scalar!(
            "INSERT INTO videos (title, description, filename, original_filename, file_size, status, user_id) 
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
            request.title,
            request.description as Option<String>,
            filename,
            original_filename,
            file_size,
            VideoStatus::Uploading.to_string(),
            user_id
        )
        .fetch_one(&self.pool)
        .await?;

        // Fetch the complete video record
        let video = sqlx::query_as!(Video, "SELECT * FROM videos WHERE id = $1", video_id)
            .fetch_one(&self.pool)
            .await?;

        Ok(video)
    }

    async fn get_video_by_id(&self, video_id: &Uuid) -> Result<Option<Video>> {
        let video = sqlx::query_as!(Video, "SELECT * FROM videos WHERE id = $1", video_id)
            .fetch_optional(&self.pool)
            .await?;

        Ok(video)
    }

    async fn list_videos(
        &self,
        user_id: Option<Uuid>,
        limit: i64,
        offset: i64,
    ) -> Result<PaginatedResponse<Video>> {
        let videos = if let Some(user_id) = user_id {
            sqlx::query_as!(
                Video,
                "SELECT * FROM videos WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                user_id,
                limit,
                offset
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query_as!(
                Video,
                "SELECT * FROM videos ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                limit,
                offset
            )
            .fetch_all(&self.pool)
            .await?
        };

        let total = if let Some(user_id) = user_id {
            sqlx::query_scalar!("SELECT COUNT(*) FROM videos WHERE user_id = $1", user_id)
                .fetch_one(&self.pool)
                .await?
        } else {
            sqlx::query_scalar!("SELECT COUNT(*) FROM videos")
                .fetch_one(&self.pool)
                .await?
        };

        let final_total = total.unwrap_or(0);
        let total_pages = (final_total + limit - 1) / limit;
        let current_page = (offset / limit) + 1;

        Ok(PaginatedResponse {
            data: videos,
            pagination: PaginationMeta {
                total: final_total,
                limit,
                offset,
                current_page,
                total_pages,
                has_next: current_page < total_pages,
                has_previous: current_page > 1,
            },
        })
    }

    async fn update_video_status(&self, video_id: &Uuid, status: VideoStatus) -> Result<()> {
        sqlx::query!(
            "UPDATE videos SET status = $1, updated_at = NOW() WHERE id = $2",
            status.to_string(),
            video_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn update_video_metadata(
        &self,
        video_id: &Uuid,
        duration: Option<i32>,
        thumbnail_path: Option<String>,
        hls_playlist_path: Option<String>,
    ) -> Result<()> {
        log::info!("🚀 Updating video metadata for video_id: {}", video_id);
        log::info!("🔹 Duration: {:?}", duration);
        log::info!("🔹 Thumbnail path: {:?}", thumbnail_path);
        log::info!("🔹 HLS playlist path: {:?}", hls_playlist_path);

        let result = sqlx::query!(
            "UPDATE videos SET duration = $1, thumbnail_path = $2, hls_playlist_path = $3, updated_at = NOW() WHERE id = $4",
            duration,
            thumbnail_path,
            hls_playlist_path,
            video_id
        )
        .execute(&self.pool)
        .await?;

        log::info!(
            "✅ Database update result: {} rows affected",
            result.rows_affected()
        );

        if result.rows_affected() == 0 {
            log::warn!(
                "⚠️ No rows were updated! Video ID {} might not exist",
                video_id
            );
        }

        Ok(())
    }

    async fn delete_video(&self, video_id: &Uuid, user_id: &Uuid) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM videos WHERE id = $1 AND user_id = $2",
            video_id,
            user_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn update_video_details(
        &self,
        video_id: &Uuid,
        title: String,
        description: Option<String>,
    ) -> Result<Video> {
        let updated_video = sqlx::query_as!(
            Video,
            "UPDATE videos SET title = $1, description = $2, updated_at = NOW() WHERE id = $3 RETURNING *",
            title,
            description,
            video_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(updated_video)
    }
}
