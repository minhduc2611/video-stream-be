use actix_web::{web, HttpResponse, Result};
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use sqlx::PgPool;
use uuid::Uuid;
use std::env;

use crate::models::{CreateVideoRequest, VideoResponse, PaginatedResponse, HlsStreamingResponse, VideoUploadResponse};
use crate::services::{VideoService, StorageService};
use crate::utils::response::ApiResponse;

pub async fn list_videos(
    pool: web::Data<PgPool>,
    user_id: web::ReqData<Uuid>,
    query: web::Query<VideoListQuery>,
) -> Result<HttpResponse> {
    let video_service = VideoService::new(pool.get_ref().clone());
    
    let limit = query.limit.unwrap_or(10).min(100); // Max 100 items per page
    let offset = query.offset.unwrap_or(0);
    
    match video_service.list_videos(Some(user_id.into_inner()), limit, offset).await {
        Ok(videos) => Ok(HttpResponse::Ok().json(ApiResponse::success(videos))),
        Err(e) => {
            log::error!("List videos error: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::error(
                "Failed to fetch videos",
                None,
            )))
        }
    }
}

pub async fn get_video(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_id = path.into_inner();
    let video_service = VideoService::new(pool.get_ref().clone());

    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => Ok(HttpResponse::Ok().json(ApiResponse::success(VideoResponse::from(video)))),
        Ok(None) => Ok(HttpResponse::NotFound().json(ApiResponse::error(
            "Video not found",
            None,
        ))),
        Err(e) => {
            log::error!("Get video error: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::error(
                "Failed to fetch video",
                None,
            )))
        }
    }
}

pub async fn upload_video(
    pool: web::Data<PgPool>,
    user_id: web::ReqData<Uuid>,
    mut payload: Multipart,
) -> Result<HttpResponse> {
    let video_service = VideoService::new(pool.get_ref().clone());
    let upload_dir = env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string());
    let storage_service = StorageService::new(upload_dir);

    let mut title = String::new();
    let mut description = None;
    let mut hls_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut video_id = Uuid::new_v4();

    // Parse multipart form data
    while let Some(mut field) = payload.try_next().await? {
        match field.name() {
            "title" => {
                let mut data = Vec::new();
                while let Some(chunk) = field.try_next().await? {
                    data.extend_from_slice(&chunk);
                }
                title = String::from_utf8_lossy(&data).to_string();
            }
            "description" => {
                let mut data = Vec::new();
                while let Some(chunk) = field.try_next().await? {
                    data.extend_from_slice(&chunk);
                }
                let desc = String::from_utf8_lossy(&data).to_string();
                if !desc.is_empty() {
                    description = Some(desc);
                }
            }
            "files" => {
                let filename = field.content_disposition().get_filename()
                    .unwrap_or("unknown")
                    .to_string();
                
                let mut file_data = Vec::new();
                while let Some(chunk) = field.try_next().await? {
                    file_data.extend_from_slice(&chunk);
                }
                
                // Validate HLS file types
                if is_hls_file(&filename) {
                    hls_files.push((filename, file_data));
                }
            }
            _ => {}
        }
    }

    if title.is_empty() || hls_files.is_empty() {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<String>::error(
            "Title and HLS files are required",
            None,
        )));
    }

    // Find the master playlist file
    let master_playlist = hls_files.iter()
        .find(|(filename, _)| filename.ends_with("master.m3u8") || filename.ends_with("playlist.m3u8"));

    if master_playlist.is_none() {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<String>::error(
            "Master playlist file (playlist.m3u8) is required",
            None,
        )));
    }

    // Create video record
    let create_request = CreateVideoRequest { title, description };
    let total_size: i64 = hls_files.iter().map(|(_, data)| data.len() as i64).sum();
    
    match video_service.create_video(
        create_request,
        user_id.into_inner(),
        "playlist.m3u8".to_string(),
        "hls_video".to_string(),
        total_size,
    ).await {
        Ok(video) => {
            // Save all HLS files to storage
            for (filename, file_data) in hls_files {
                let file_path = storage_service.get_hls_file_path(&video_id, &filename);
                if let Err(e) = storage_service.save_uploaded_file(&video_id, &filename, &file_data).await {
                    log::error!("Failed to save HLS file {}: {}", filename, e);
                    return Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                        "Failed to save HLS files",
                        None,
                    )));
                }
            }

            // Update video with HLS playlist path
            if let Err(e) = video_service.update_video_metadata(
                &video_id,
                None, // Duration will be extracted from playlist
                None, // Thumbnail can be added later
                Some(format!("hls/{}/playlist.m3u8", video_id)),
            ).await {
                log::error!("Failed to update video metadata: {}", e);
            }

            // Update status to ready
            if let Err(e) = video_service.update_video_status(&video_id, crate::models::VideoStatus::Ready).await {
                log::error!("Failed to update video status: {}", e);
            }

            let upload_response = VideoUploadResponse {
                video_id: video.id,
                title: video.title.clone(),
                description: video.description.clone(),
                status: video.status.clone(),
                hls_files_count: hls_files.len(),
                total_size,
                created_at: video.created_at,
            };

            Ok(HttpResponse::Created().json(ApiResponse::success(upload_response)))
        }
        Err(e) => {
            log::error!("Create video error: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Failed to create video",
                None,
            )))
        }
    }
}

pub async fn stream_video(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_id = path.into_inner();
    let video_service = VideoService::new(pool.get_ref().clone());

    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            if let Some(hls_path) = video.hls_playlist_path {
                // Return HLS streaming URLs for frontend
                let streaming_response = HlsStreamingResponse {
                    video_id,
                    hls_url: format!("/api/v1/videos/{}/stream/playlist.m3u8", video_id),
                    thumbnail_url: video.thumbnail_path.map(|_| format!("/api/v1/videos/{}/thumbnail", video_id)),
                    status: video.status.clone(),
                    title: video.title.clone(),
                    duration: video.duration,
                };

                Ok(HttpResponse::Ok().json(ApiResponse::success(streaming_response)))
            } else {
                Ok(HttpResponse::ServiceUnavailable().json(ApiResponse::<String>::error(
                    "Video is still processing",
                    None,
                )))
            }
        }
        Ok(None) => Ok(HttpResponse::NotFound().json(ApiResponse::<String>::error(
            "Video not found",
            None,
        ))),
        Err(e) => {
            log::error!("Stream video error: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Failed to get video",
                None,
            )))
        }
    }
}

pub async fn get_thumbnail(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_id = path.into_inner();
    let video_service = VideoService::new(pool.get_ref().clone());

    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            if let Some(thumbnail_path) = video.thumbnail_path {
                Ok(HttpResponse::Ok().json(ApiResponse::success(serde_json::json!({
                    "thumbnail_url": format!("/api/v1/videos/{}/thumbnail.jpg", video_id)
                }))))
            } else {
                Ok(HttpResponse::NotFound().json(ApiResponse::error(
                    "Thumbnail not available",
                    None,
                )))
            }
        }
        Ok(None) => Ok(HttpResponse::NotFound().json(ApiResponse::error(
            "Video not found",
            None,
        ))),
        Err(e) => {
            log::error!("Get thumbnail error: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::error(
                "Failed to get thumbnail",
                None,
            )))
        }
    }
}

pub async fn delete_video(
    pool: web::Data<PgPool>,
    user_id: web::ReqData<Uuid>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_id = path.into_inner();
    let video_service = VideoService::new(pool.get_ref().clone());

    match video_service.delete_video(&video_id, &user_id.into_inner()).await {
        Ok(true) => Ok(HttpResponse::Ok().json(ApiResponse::success("Video deleted successfully"))),
        Ok(false) => Ok(HttpResponse::NotFound().json(ApiResponse::error(
            "Video not found or access denied",
            None,
        ))),
        Err(e) => {
            log::error!("Delete video error: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::error(
                "Failed to delete video",
                None,
            )))
        }
    }
}

#[derive(serde::Deserialize)]
pub struct VideoListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn is_hls_file(filename: &str) -> bool {
    let extension = filename.split('.').last().unwrap_or("").to_lowercase();
    matches!(extension.as_str(), "m3u8" | "ts")
}

pub async fn serve_hls_file(
    pool: web::Data<PgPool>,
    path: web::Path<(Uuid, String)>,
) -> Result<HttpResponse> {
    let (video_id, filename) = path.into_inner();
    let video_service = VideoService::new(pool.get_ref().clone());
    let upload_dir = env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string());
    let storage_service = StorageService::new(upload_dir);

    // Verify video exists and is ready
    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            if video.status != crate::models::VideoStatus::Ready {
                return Ok(HttpResponse::ServiceUnavailable().json(ApiResponse::<String>::error(
                    "Video is not ready for streaming",
                    None,
                )));
            }

            // Validate filename is HLS file
            if !is_hls_file(&filename) {
                return Ok(HttpResponse::BadRequest().json(ApiResponse::<String>::error(
                    "Invalid file type",
                    None,
                )));
            }

            let file_path = storage_service.get_hls_file_path(&video_id, &filename);
            
            // Check if file exists
            if !storage_service.file_exists(&file_path).await {
                return Ok(HttpResponse::NotFound().json(ApiResponse::<String>::error(
                    "File not found",
                    None,
                )));
            }

            // Read and serve file
            match tokio::fs::read(&file_path).await {
                Ok(file_data) => {
                    let content_type = if filename.ends_with(".m3u8") {
                        "application/vnd.apple.mpegurl"
                    } else {
                        "video/mp2t"
                    };

                    Ok(HttpResponse::Ok()
                        .content_type(content_type)
                        .append_header(("Access-Control-Allow-Origin", "*"))
                        .append_header(("Access-Control-Allow-Methods", "GET"))
                        .append_header(("Access-Control-Allow-Headers", "Content-Type"))
                        .body(file_data))
                }
                Err(e) => {
                    log::error!("Failed to read HLS file {}: {}", file_path, e);
                    Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                        "Failed to read file",
                        None,
                    )))
                }
            }
        }
        Ok(None) => Ok(HttpResponse::NotFound().json(ApiResponse::<String>::error(
            "Video not found",
            None,
        ))),
        Err(e) => {
            log::error!("Error getting video: {}", e);
            Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Internal server error",
                None,
            )))
        }
    }
}

fn is_video_file(filename: &str) -> bool {
    let extension = filename.split('.').last().unwrap_or("").to_lowercase();
    matches!(extension.as_str(), "mp4" | "avi" | "mov" | "wmv" | "flv" | "webm" | "mkv")
}
