use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Video {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub filename: String,
    pub original_filename: String,
    pub file_size: i64,
    pub duration: Option<i32>, // Duration in seconds
    pub thumbnail_path: Option<String>,
    pub hls_playlist_path: Option<String>,
    pub status: VideoStatus,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "video_status", rename_all = "lowercase")]
pub enum VideoStatus {
    Uploading,
    Processing,
    Ready,
    Failed,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CreateVideoRequest {
    #[validate(length(min = 1, max = 200))]
    pub title: String,
    #[validate(length(max = 1000))]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HlsStreamingResponse {
    pub video_id: Uuid,
    pub hls_url: String,
    pub thumbnail_url: Option<String>,
    pub status: VideoStatus,
    pub title: String,
    pub duration: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct VideoUploadResponse {
    pub video_id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub status: VideoStatus,
    pub hls_files_count: usize,
    pub total_size: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct VideoResponse {
    pub id: Uuid,
    pub title: String,
    pub description: Option<String>,
    pub filename: String,
    pub file_size: i64,
    pub duration: Option<i32>,
    pub thumbnail_path: Option<String>,
    pub hls_playlist_path: Option<String>,
    pub hls_stream_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub status: VideoStatus,
    pub user_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct PaginatedResponse<T> {
    pub data: Vec<T>,
    pub pagination: PaginationMeta,
}

#[derive(Debug, Serialize)]
pub struct PaginationMeta {
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub current_page: i64,
    pub total_pages: i64,
    pub has_next: bool,
    pub has_previous: bool,
}

impl From<Video> for VideoResponse {
    fn from(video: Video) -> Self {
        let hls_stream_url = video.hls_playlist_path.as_ref().map(|_| {
            format!("/api/v1/videos/{}/stream/playlist.m3u8", video.id)
        });
        
        let thumbnail_url = video.thumbnail_path.as_ref().map(|_| {
            format!("/api/v1/videos/{}/thumbnail", video.id)
        });

        VideoResponse {
            id: video.id,
            title: video.title,
            description: video.description,
            filename: video.filename,
            file_size: video.file_size,
            duration: video.duration,
            thumbnail_path: video.thumbnail_path,
            hls_playlist_path: video.hls_playlist_path,
            hls_stream_url,
            thumbnail_url,
            status: video.status,
            user_id: video.user_id,
            created_at: video.created_at,
            updated_at: video.updated_at,
        }
    }
}
