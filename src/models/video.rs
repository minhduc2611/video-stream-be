use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use validator::Validate;
use std::str::FromStr;

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
    #[sqlx(rename = "status")]
    pub status: Option<String>, // Store as string for SQLx compatibility
    pub user_id: Uuid,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

impl Video {
    pub fn get_status(&self) -> VideoStatus {
        self.status.as_ref()
            .and_then(|s| VideoStatus::from_str(s).ok())
            .unwrap_or(VideoStatus::Failed)
    }
    
    pub fn set_status(&mut self, status: VideoStatus) {
        self.status = Some(status.to_string());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VideoStatus {
    Uploading,
    Processing,
    Ready,
    Failed,
}

impl FromStr for VideoStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "uploading" => Ok(VideoStatus::Uploading),
            "processing" => Ok(VideoStatus::Processing),
            "ready" => Ok(VideoStatus::Ready),
            "failed" => Ok(VideoStatus::Failed),
            _ => Err(format!("Invalid video status: {}", s)),
        }
    }
}

impl std::fmt::Display for VideoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VideoStatus::Uploading => write!(f, "uploading"),
            VideoStatus::Processing => write!(f, "processing"),
            VideoStatus::Ready => write!(f, "ready"),
            VideoStatus::Failed => write!(f, "failed"),
        }
    }
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

impl VideoResponse {
    pub fn from_video_with_gcs_urls(video: Video, gcs_service: &crate::services::GcsService) -> Self {
        let status = video.get_status();
        
        let hls_stream_url = video.hls_playlist_path.as_ref().map(|path| {
            gcs_service.get_public_url(path)
        });
        
        let thumbnail_url = video.thumbnail_path.as_ref().map(|path| {
            gcs_service.get_public_url(path)
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
            status,
            user_id: video.user_id,
            created_at: video.created_at.unwrap_or_default(),
            updated_at: video.updated_at.unwrap_or_default(),
        }
    }
}

impl From<Video> for VideoResponse {
    fn from(video: Video) -> Self {
        let video_id = video.id;
        let status = video.get_status();
        
        let hls_stream_url = video.hls_playlist_path.as_ref().map(|_| {
            format!("/api/v1/videos/{}/stream/playlist.m3u8", video_id)
        });
        
        let thumbnail_url = video.thumbnail_path.as_ref().map(|_| {
            format!("/api/v1/videos/{}/thumbnail", video_id)
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
            status,
            user_id: video.user_id,
            created_at: video.created_at.unwrap_or_default(),
            updated_at: video.updated_at.unwrap_or_default(),
        }
    }
}
