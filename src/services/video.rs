use crate::models::{Video, CreateVideoRequest, VideoResponse, VideoStatus, PaginatedResponse, PaginationMeta};
use sqlx::PgPool;
use uuid::Uuid;
use anyhow::Result;

pub struct VideoService {
    pool: PgPool,
}

impl VideoService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create_video(&self, request: CreateVideoRequest, user_id: Uuid, filename: String, original_filename: String, file_size: i64) -> Result<Video> {
        let video = sqlx::query_as!(
            Video,
            "INSERT INTO videos (title, description, filename, original_filename, file_size, status, user_id) 
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
            request.title,
            request.description,
            filename,
            original_filename,
            file_size,
            VideoStatus::Uploading as _,
            user_id
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(video)
    }

    pub async fn get_video_by_id(&self, video_id: &Uuid) -> Result<Option<Video>> {
        let video = sqlx::query_as!(
            Video,
            "SELECT * FROM videos WHERE id = $1",
            video_id
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(video)
    }

    pub async fn list_videos(&self, user_id: Option<Uuid>, limit: i64, offset: i64) -> Result<PaginatedResponse<VideoResponse>> {
        let query = if let Some(user_id) = user_id {
            "SELECT * FROM videos WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2 OFFSET $3"
        } else {
            "SELECT * FROM videos ORDER BY created_at DESC LIMIT $1 OFFSET $2"
        };

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
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM videos WHERE user_id = $1",
                user_id
            )
            .fetch_one(&self.pool)
            .await?
        } else {
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM videos"
            )
            .fetch_one(&self.pool)
            .await?
        };

        let total_pages = (total + limit - 1) / limit;
        let current_page = (offset / limit) + 1;

        Ok(PaginatedResponse {
            data: videos.into_iter().map(|v| v.into()).collect(),
            pagination: PaginationMeta {
                total,
                limit,
                offset,
                current_page,
                total_pages,
                has_next: current_page < total_pages,
                has_previous: current_page > 1,
            },
        })
    }

    pub async fn update_video_status(&self, video_id: &Uuid, status: VideoStatus) -> Result<()> {
        sqlx::query!(
            "UPDATE videos SET status = $1, updated_at = NOW() WHERE id = $2",
            status as _,
            video_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_video_metadata(&self, video_id: &Uuid, duration: Option<i32>, thumbnail_path: Option<String>, hls_playlist_path: Option<String>) -> Result<()> {
        sqlx::query!(
            "UPDATE videos SET duration = $1, thumbnail_path = $2, hls_playlist_path = $3, updated_at = NOW() WHERE id = $4",
            duration,
            thumbnail_path,
            hls_playlist_path,
            video_id
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn delete_video(&self, video_id: &Uuid, user_id: &Uuid) -> Result<bool> {
        let result = sqlx::query!(
            "DELETE FROM videos WHERE id = $1 AND user_id = $2",
            video_id,
            user_id
        )
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}
