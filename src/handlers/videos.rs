use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse, Result};
use futures_util::TryStreamExt;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;
use validator::Validate;

use crate::app_state::AppState;
use crate::models::{
    CreateVideoRequest, HlsStreamingResponse, UpdateVideoRequest, VideoResponse,
    VideoUploadResponse,
};
use crate::services::VideoProcessingService;
use crate::utils::response::ApiResponse;

pub async fn upload_video(
    req: HttpRequest,
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    mut payload: Multipart,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);
    let processing_service = Arc::clone(&app_state.video_processing_service);
    let metrics_service = Arc::clone(&app_state.metrics_service);
    let handler_timer = Instant::now();
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

    log::info!("Queuing video upload and processing tasks");

    let upload_metadata = json!({
        "video_id": video_id,
        "user_id": user_id_value,
        "filename": filename,
        "original_filename": original_filename,
        "file_size": file_size,
        "route": req.path(),
        "method": req.method().as_str(),
    });

    let upload_benchmark_run_id = match metrics_service
        .create_benchmark_run("video_upload", Some(upload_metadata))
        .await
    {
        Ok(id) => Some(id),
        Err(err) => {
            log::warn!(
                "Failed to create upload benchmark run for {}: {}",
                video_id,
                err
            );
            None
        }
    };

    if let Err(err) = metrics_service
        .record_video_processing_step(
            upload_benchmark_run_id,
            Some(video_id),
            "create_video_record",
            None,
            None,
            None,
        )
        .await
    {
        log::warn!(
            "Failed to record create_video_record metric for {}: {}",
            video_id,
            err
        );
    }

    let file_data_for_processing = file_data.clone();
    let job_video_service = Arc::clone(&video_service);
    let job_storage_service = Arc::clone(&storage_service);
    let job_processing_service = Arc::clone(&processing_service);
    let job_metrics_service = Arc::clone(&metrics_service);
    let job_filename = filename.clone();
    let job_user_id = user_id_value;
    let job_video_id = video_id;
    let job_file_data = file_data;
    let job_benchmark_run_id = upload_benchmark_run_id;

    actix_web::rt::spawn(async move {
        let storage_video_path = job_storage_service.get_video_path(&job_video_id, &job_filename);

        let upload_timer = Instant::now();
        if let Err(e) = job_storage_service
            .upload_file_data(job_file_data, &storage_video_path)
            .await
        {
            log::error!("Failed to upload video file to storage: {}", e);
            let _ = job_video_service
                .delete_video(&job_video_id, &job_user_id)
                .await;
            if let Err(err) = job_metrics_service
                .record_video_processing_step(
                    job_benchmark_run_id,
                    Some(job_video_id),
                    "upload_original_video_failed",
                    Some(upload_timer.elapsed().as_millis() as i64),
                    None,
                    None,
                )
                .await
            {
                log::warn!(
                    "Failed to record upload_original_video_failed metric for {}: {}",
                    job_video_id,
                    err
                );
            }
            return;
        }
        if let Err(err) = job_metrics_service
            .record_video_processing_step(
                job_benchmark_run_id,
                Some(job_video_id),
                "upload_original_video",
                Some(upload_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record upload_original_video metric for {}: {}",
                job_video_id,
                err
            );
        }

        log::info!("Starting video processing task for {}", job_video_id);
        if let Err(e) = job_processing_service
            .process_video(
                job_video_id,
                file_data_for_processing,
                &job_filename,
                job_benchmark_run_id,
            )
            .await
        {
            log::error!("Failed to start video processing: {}", e);
            if let Err(update_err) = job_video_service
                .update_video_status(&job_video_id, crate::models::VideoStatus::Failed)
                .await
            {
                log::error!(
                    "Failed to update video status to failed for {}: {}",
                    job_video_id,
                    update_err
                );
            }
        }
    });

    let response = VideoUploadResponse {
        video_id,
        title,
        description,
        status: crate::models::VideoStatus::Uploading,
        hls_files_count: 0,
        total_size: file_size as i64,
        created_at: video.created_at.unwrap_or_default(),
    };

    let http_response = HttpResponse::Accepted().json(ApiResponse::success(response));

    if let Err(err) = metrics_service
        .record_video_processing_step(
            upload_benchmark_run_id,
            Some(video_id),
            "upload_handler_complete",
            Some(handler_timer.elapsed().as_millis() as i64),
            None,
            None,
        )
        .await
    {
        log::warn!(
            "Failed to record upload_handler_complete metric for {}: {}",
            video_id,
            err
        );
    }

    Ok(http_response)
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

    // Get video details before deletion to confirm ownership
    match video_service.get_video_by_id(&video_id).await {
        Ok(Some(video)) => {
            if video.user_id != user_id_value {
                return Ok(HttpResponse::Forbidden()
                    .json(ApiResponse::<String>::error("Access denied", None)));
            }
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

    let folder_prefix = video_id.to_string();
    if let Err(e) = storage_service.delete_folder(&folder_prefix).await {
        log::warn!(
            "Failed to delete storage folder for video {}: {}",
            video_id,
            e
        );
    }

    Ok(HttpResponse::Ok().json(ApiResponse::success("Video deleted successfully")))
}

/// Update video details (title and description)
pub async fn update_video(
    app_state: web::Data<AppState>,
    user_id: web::ReqData<Uuid>,
    path: web::Path<Uuid>,
    payload: web::Json<UpdateVideoRequest>,
) -> Result<HttpResponse> {
    let video_service = Arc::clone(&app_state.video_service);
    let storage_service = Arc::clone(&app_state.storage_service);

    let user_id_value = user_id.into_inner();
    let video_id = path.into_inner();
    let update_request = payload.into_inner();

    if update_request.title.is_none() && update_request.description.is_none() {
        return Ok(
            HttpResponse::BadRequest().json(ApiResponse::<String>::error(
                "At least one field (title or description) must be provided",
                None,
            )),
        );
    }

    if let Err(validation_errors) = update_request.validate() {
        let mut error_map: HashMap<String, Vec<String>> = HashMap::new();

        for (field, errors) in validation_errors.field_errors() {
            let codes = errors
                .iter()
                .map(|error| error.code.to_string())
                .collect::<Vec<String>>();
            error_map.insert(field.to_string(), codes);
        }

        return Ok(
            HttpResponse::BadRequest().json(ApiResponse::<String>::error(
                "Invalid data provided",
                Some(error_map),
            )),
        );
    }

    let existing_video = match video_service.get_video_by_id(&video_id).await {
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
            log::error!("Failed to fetch video for update: {}", e);
            return Ok(HttpResponse::InternalServerError()
                .json(ApiResponse::<String>::error("Failed to fetch video", None)));
        }
    };

    let UpdateVideoRequest { title, description } = update_request;

    let updated_title = title
        .and_then(|t| {
            let trimmed = t.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })
        .unwrap_or_else(|| existing_video.title.clone());

    let final_description = match description {
        Some(desc) => {
            let trimmed = desc.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        None => existing_video.description.clone(),
    };

    match video_service
        .update_video_details(&video_id, updated_title, final_description)
        .await
    {
        Ok(updated_video) => {
            let response =
                VideoResponse::from_video_with_storage(updated_video, storage_service.as_ref());
            Ok(HttpResponse::Ok().json(ApiResponse::success(response)))
        }
        Err(e) => {
            log::error!("Failed to update video details: {}", e);
            Ok(HttpResponse::InternalServerError()
                .json(ApiResponse::<String>::error("Failed to update video", None)))
        }
    }
}
