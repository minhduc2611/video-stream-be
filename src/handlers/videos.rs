use actix_web::{web, HttpResponse, Result};
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use sqlx::PgPool;
use uuid::Uuid;
use serde::Deserialize;

use crate::models::{CreateVideoRequest, VideoUploadResponse};
use crate::services::{
    VideoService, 
    GcsService,
    VideoProcessingService
};
use crate::utils::response::ApiResponse;

pub async fn upload_video(
    pool: web::Data<PgPool>,
    user_id: web::ReqData<Uuid>,
    mut payload: Multipart,
) -> Result<HttpResponse> {
    let video_service = VideoService::new(pool.get_ref().clone());
    let gcs_service = match GcsService::new().await {
        Ok(service) => service,
        Err(e) => {
            log::error!("Failed to initialize GCS service: {}", e);
            return Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Storage service unavailable",
                None,
            )));
        }
    };
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
                let filename = field.content_disposition().get_filename()
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
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<String>::error(
            "Title is required",
            None,
        )));
    }

    if video_file.is_none() {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<String>::error(
            "Video file is required",
            None,
        )));
    }

    let (original_filename, file_data) = video_file.unwrap();
    let file_size = file_data.len() as u64;

    // Validate video file
    log::info!("Validating video file: {}", original_filename);
    if let Err(e) = VideoProcessingService::validate_video_file(&original_filename, file_size) {
        return Ok(HttpResponse::BadRequest().json(ApiResponse::<String>::error(
            &e.to_string(),
            None,
        )));
    }

    // Check FFmpeg availability
    log::info!("Checking FFmpeg availability");
    if let Err(e) = VideoProcessingService::check_ffmpeg_availability() {
        log::error!("FFmpeg not available: {}", e);
        return Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
            "Video processing service is not available",
            None,
        )));
    }

    // Generate unique filename
    log::info!("Generating unique filename");
    let video_id = Uuid::new_v4();
    let file_extension = original_filename.split('.').last().unwrap_or("mp4");
    let filename = format!("{}.{}", video_id, file_extension);

    // Create video record in database
    let create_request = CreateVideoRequest { 
        title: title.clone(), 
        description: description.clone() 
    };

    log::info!("Creating video record in database");
    let video = match video_service.create_video(
        create_request,
        user_id_value,
        filename.clone(),
        original_filename.clone(),
        file_size as i64,
    ).await {
        Ok(video) => video,
        Err(e) => {
            log::error!("Failed to create video record: {}", e);
            return Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
                "Failed to create video record",
                None,
            )));
        }
    };

    log::info!("Saving video file to GCS");
    // Save video file to GCS directly from memory
    let gcs_video_path = gcs_service.get_video_path(&video_id, &filename);
    
    // Clone file data for processing since we need it for both GCS upload and processing
    let file_data_for_processing = file_data.clone();
    
    if let Err(e) = gcs_service.upload_file_data(file_data, &gcs_video_path).await {
        log::error!("Failed to upload video file to GCS: {}", e);
        // Clean up database record
        let _ = video_service.delete_video(&video_id, &user_id_value).await;
        return Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
            "Failed to upload video file",
            None,
        )));
    }

    log::info!("Starting video processing in background");
    // Start video processing in background
    let processing_service = VideoProcessingService::new(
        video_service.clone(),
        gcs_service.clone(),
    );

    log::info!("Processing video");
    if let Err(e) = processing_service.process_video(video_id, file_data_for_processing, &filename).await {
        log::error!("Failed to start video processing: {}", e);
        // Update status to failed
        let _ = video_service.update_video_status(&video_id, crate::models::VideoStatus::Failed).await;
        return Ok(HttpResponse::InternalServerError().json(ApiResponse::<String>::error(
            "Failed to start video processing",
            None,
        )));
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
    matches!(extension.as_str(), "mp4" | "mov" | "avi" | "mkv" | "webm" | "flv" | "wmv" | "m4v")
}