use actix_multipart::Multipart;
use actix_web::{web, HttpResponse, Result};
use futures_util::TryStreamExt;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::models::{CreateVideoRequest, HlsStreamingResponse, VideoResponse, VideoUploadResponse};
use crate::services::{CloudStorageService, VideoProcessingService};
use crate::utils::response::ApiResponse;

pub async fn upload_video(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    mut payload: Multipart,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);
    let processing_service = Arc::clone(&app_state.video_processing_service);
    let user_id_value = user_id.into_inner();

    let mut title = String::new();
    let mut description = None;
    let mut video_file: Option<(String, Vec<u8>)> = None;

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
                let filename = field
                    .content_disposition()
                    .get_filename()
                    .unwrap_or("unknown")
                    .to_string();

                let mut file_data = Vec::new();
                while let Some(chunk) = field.try_next().await? {
                    file_data.extend_from_slice(&chunk);
                }

                // Validate video file types
                if is_video_file(&filename) {
                    video_file = Some((filename, file_data));
                }
            }
            _ => {}
        }
    }

    // Validate required fields
    if title.is_empty() {
        return Ok(HttpResponse::BadRequest()
            .json(ApiResponse::<String>::error("Title is required", None)));
    }

    if video_file.is_none() {
        return Ok(HttpResponse::BadRequest()
            .json(ApiResponse::<String>::error("Video file is required", None)));
    }

    let (original_filename, file_data) = video_file.unwrap();
    let file_size = file_data.len() as u64;

    // Validate video file
    log::info!("Validating video file: {}", original_filename);
    if let Err(e) = VideoProcessingService::validate_video_file(&original_filename, file_size) {
        return Ok(
            HttpResponse::BadRequest().json(ApiResponse::<String>::error(&e.to_string(), None))
        );
    }

    // Check FFmpeg availability
    log::info!("Checking FFmpeg availability");
    if let Err(e) = VideoProcessingService::check_ffmpeg_availability() {
        log::error!("FFmpeg not available: {}", e);
        return Ok(
            HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Video processing service is not available",
                None,
            )),
        );
    }

    // Generate unique filename
    log::info!("Generating unique filename");
    // let video_id = Uuid::new_v4();
    let file_extension = original_filename.split('.').last().unwrap_or("mp4");
    let filename = format!(
        "{}.{}",
        title.to_lowercase().replace(" ", "_").replace(".", ""),
        file_extension
    );

    // Create video record in database
    let create_request = CreateVideoRequest {
        title: title.clone(),
        description: description.clone(),
    };

    log::info!("Creating video record in database");
    let video = match video_service
        .create_video(
            create_request,
            user_id_value,
            filename.clone(),
            original_filename.clone(),
            file_size as i64,
        )
        .await
    {
        Ok(video) => video,
        Err(e) => {
            log::error!("Failed to create video record: {}", e);
            return Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                    "Failed to create video record",
                    None,
                )),
            );
        }
    };

    let video_id: Uuid = video.id;

    log::info!("Saving video file to GCS");
    // Save video file to GCS directly from memory
    let storage_video_path = storage_service.get_video_path(&video_id, &filename);

    // Clone file data for processing since we need it for both GCS upload and processing
    let file_data_for_processing = file_data.clone();

    if let Err(e) = storage_service
        .upload_file_data(file_data, &storage_video_path)
        .await
    {
        log::error!("Failed to upload video file to storage: {}", e);
        // Clean up database record
        let _ = video_service.delete_video(&video_id, &user_id_value).await;
        return Ok(
            HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Failed to upload video file",
                None,
            )),
        );
    }

    log::info!("Starting video processing in background");
    // Start video processing in background
    log::info!("Processing video");
    if let Err(e) = processing_service
        .process_video(video_id, file_data_for_processing, &filename)
        .await
    {
        log::error!("Failed to start video processing: {}", e);
        // Update status to failed
        let _ = video_service
            .update_video_status(&video_id, crate::models::VideoStatus::Failed)
            .await;
        return Ok(
            HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Failed to start video processing",
                None,
            )),
        );
    }

    // Return success response
    let response = VideoUploadResponse {
        video_id,
        title,
        description,
        status: crate::models::VideoStatus::Processing,
        hls_files_count: 0, // Will be updated after processing
        total_size: file_size as i64,
        created_at: video.created_at.unwrap_or_default(),
    };

    Ok(HttpResponse::Ok().json(ApiResponse::success(response)))
}

// Video list query parameters
#[derive(Debug, Deserialize)]
pub struct VideoListQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

fn is_video_file(filename: &str) -> bool {
    let extension = filename.split('.').last().unwrap_or("").to_lowercase();
    matches!(
        extension.as_str(),
        "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" | "m4v"
    )
}

/// List videos with pagination
pub async fn list_videos(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    query: web::Query<VideoListQuery>,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);
    let user_id_value = user_id.into_inner();

    let limit = query.limit.unwrap_or(10).min(100); // Max 100 per page
    let offset = query.offset.unwrap_or(0);

    match video_service
        .list_videos(Some(user_id_value), limit, offset)
        .await
    {
        Ok(result) => {
            let paginated = crate::models::PaginatedResponse {
                data: result
                    .data
                    .into_iter()
                    .map(|video| {
                        VideoResponse::from_video_with_storage(video, storage_service.as_ref())
                    })
                    .collect(),
                pagination: result.pagination,
            };
            Ok(HttpResponse::Ok().json(ApiResponse::success(paginated)))
        }
        Err(e) => {
            log::error!("Failed to list videos: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(ApiResponse::<String>::error("Failed to fetch videos", None)))
        }
    }
}

/// Get video details by ID
pub async fn get_video(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);
    let user_id_value = user_id.into_inner();
    let video_id = path.into_inner();

    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            // Check if user owns the video
            if video.user_id != user_id_value {
                return Ok(HttpResponse::Forbidden()
                    .json(ApiResponse::<String>::error("Access denied", None)));
            }

            let video_response =
                VideoResponse::from_video_with_storage(video, storage_service.as_ref());
            Ok(HttpResponse::Ok().json(ApiResponse::success(video_response)))
        }
        Ok(None) => Ok(
            HttpResponse::NotFound().json(ApiResponse::<String>::error("Video not found", None))
        ),
        Err(e) => {
            log::error!("Failed to get video: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(ApiResponse::<String>::error("Failed to fetch video", None)))
        }
    }
}

/// Get video streaming information
pub async fn stream_video(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);
    let user_id_value = user_id.into_inner();
    let video_id = path.into_inner();

    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            // Check if user owns the video
            if video.user_id != user_id_value {
                return Ok(HttpResponse::Forbidden()
                    .json(ApiResponse::<String>::error("Access denied", None)));
            }

            let status = video.get_status();
            let hls_url = if let Some(ref hls_playlist_path) = video.hls_playlist_path {
                storage_service.get_public_url(hls_playlist_path)
            } else {
                return Ok(
                    HttpResponse::BadRequest().json(ApiResponse::<String>::error(
                        "Video is not ready for streaming",
                        None,
                    )),
                );
            };

            let thumbnail_url = video
                .thumbnail_path
                .map(|path| storage_service.get_public_url(&path));

            let response = HlsStreamingResponse {
                video_id,
                hls_url,
                thumbnail_url,
                status,
                title: video.title,
                duration: video.duration,
            };

            Ok(HttpResponse::Ok().json(ApiResponse::success(response)))
        }
        Ok(None) => Ok(
            HttpResponse::NotFound().json(ApiResponse::<String>::error("Video not found", None))
        ),
        Err(e) => {
            log::error!("Failed to get video stream info: {}", e);
            Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                    "Failed to fetch video stream info",
                    None,
                )),
            )
        }
    }
}

/// Serve HLS files (.m3u8 and .ts files)
pub async fn serve_hls_file(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    path: web::Path<(Uuid, String)>,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);

    let user_id_value = user_id.into_inner();
    let (video_id, filename) = path.into_inner();

    // Verify video exists and user has access
    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            if video.user_id != user_id_value {
                return Ok(HttpResponse::Forbidden()
                    .json(ApiResponse::<String>::error("Access denied", None)));
            }

            // Check if video is ready for streaming
            if video.get_status() != crate::models::VideoStatus::Ready {
                return Ok(
                    HttpResponse::BadRequest().json(ApiResponse::<String>::error(
                        "Video is not ready for streaming",
                        None,
                    )),
                );
            }
        }
        Ok(None) => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<String>::error("Video not found", None)))
        }
        Err(e) => {
            log::error!("Failed to verify video access: {}", e);
            return Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                    "Failed to verify video access",
                    None,
                )),
            );
        }
    }

    // Construct GCS path for the HLS file
    let gcs_path = format!("hls/{}/{}", video_id, filename);

    // Download file from GCS
    let temp_path = format!("/tmp/hls_{}_{}", video_id, filename);
    if let Err(e) = storage_service.download_file(&gcs_path, &temp_path).await {
        log::error!("Failed to download HLS file from storage: {}", e);
        return Ok(
            HttpResponse::NotFound().json(ApiResponse::<String>::error("HLS file not found", None))
        );
    }

    // Determine content type based on file extension
    let content_type = if filename.ends_with(".m3u8") {
        "application/vnd.apple.mpegurl"
    } else if filename.ends_with(".ts") {
        "video/mp2t"
    } else {
        "application/octet-stream"
    };

    // Read file content
    match tokio::fs::read(&temp_path).await {
        Ok(content) => {
            // Clean up temp file
            let _ = tokio::fs::remove_file(&temp_path).await;

            Ok(HttpResponse::Ok()
                .content_type(content_type)
                .append_header(("Cache-Control", "public, max-age=3600"))
                .body(content))
        }
        Err(e) => {
            log::error!("Failed to read HLS file: {}", e);
            let _ = tokio::fs::remove_file(&temp_path).await;
            Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                    "Failed to read HLS file",
                    None,
                )),
            )
        }
    }
}

/// Get video thumbnail
pub async fn get_thumbnail(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);

    let user_id_value = user_id.into_inner();
    let video_id = path.into_inner();

    // Verify video exists and user has access
    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            if video.user_id != user_id_value {
                return Ok(HttpResponse::Forbidden()
                    .json(ApiResponse::<String>::error("Access denied", None)));
            }

            if video.thumbnail_path.is_none() {
                return Ok(HttpResponse::NotFound().json(ApiResponse::<String>::error(
                    "Thumbnail not available",
                    None,
                )));
            }
        }
        Ok(None) => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<String>::error("Video not found", None)))
        }
        Err(e) => {
            log::error!("Failed to verify video access: {}", e);
            return Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                    "Failed to verify video access",
                    None,
                )),
            );
        }
    }

    // Construct GCS path for thumbnail
    let gcs_path = format!("thumbnails/{}.jpg", video_id);

    // Download thumbnail from GCS
    let temp_path = format!("/tmp/thumb_{}.jpg", video_id);
    if let Err(e) = storage_service.download_file(&gcs_path, &temp_path).await {
        log::error!("Failed to download thumbnail from storage: {}", e);
        return Ok(HttpResponse::NotFound()
            .json(ApiResponse::<String>::error("Thumbnail not found", None)));
    }

    // Read thumbnail content
    match tokio::fs::read(&temp_path).await {
        Ok(content) => {
            // Clean up temp file
            let _ = tokio::fs::remove_file(&temp_path).await;

            Ok(HttpResponse::Ok()
                .content_type("image/jpeg")
                .append_header(("Cache-Control", "public, max-age=86400")) // Cache for 24 hours
                .body(content))
        }
        Err(e) => {
            log::error!("Failed to read thumbnail: {}", e);
            let _ = tokio::fs::remove_file(&temp_path).await;
            Ok(
                HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                    "Failed to read thumbnail",
                    None,
                )),
            )
        }
    }
}

/// Delete video
pub async fn delete_video(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    path: web::Path<Uuid>,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);

    let user_id_value = user_id.into_inner();
    let video_id = path.into_inner();

    // Get video details before deletion
    let video = match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            if video.user_id != user_id_value {
                return Ok(HttpResponse::Forbidden()
                    .json(ApiResponse::<String>::error("Access denied", None)));
            }
            video
        }
        Ok(None) => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<String>::error("Video not found", None)))
        }
        Err(e) => {
            log::error!("Failed to get video for deletion: {}", e);
            return Ok(HttpResponse::InternalServerError()
                .json(ApiResponse::<String>::error("Failed to fetch video", None)));
        }
    };

    // Delete from database
    match video_service.delete_video(&video_id, &user_id_value).await {
        Ok(true) => {
            log::info!("Video {} deleted from database", video_id);
        }
        Ok(false) => {
            return Ok(HttpResponse::NotFound()
                .json(ApiResponse::<String>::error("Video not found", None)));
        }
        Err(e) => {
            log::error!("Failed to delete video from database: {}", e);
            return Ok(HttpResponse::InternalServerError()
                .json(ApiResponse::<String>::error("Failed to delete video", None)));
        }
    }

    // Delete files from GCS (optional - files will be cleaned up by GCS lifecycle policies)
    let video_path = storage_service.get_video_path(&video_id, &video.filename);
    let _ = storage_service.delete_file(&video_path).await; // Ignore errors for cleanup

    if let Some(hls_path) = &video.hls_playlist_path {
        let _ = storage_service.delete_file(hls_path).await; // Ignore errors for cleanup
    }

    if let Some(thumbnail_path) = &video.thumbnail_path {
        let _ = storage_service.delete_file(thumbnail_path).await; // Ignore errors for cleanup
    }

    Ok(HttpResponse::Ok().json(ApiResponse::success("Video deleted successfully")))
}
