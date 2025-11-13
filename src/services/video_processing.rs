use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::fs;
use tokio::process::Command;
use uuid::Uuid;

use crate::models::VideoStatus;
use crate::services::{CloudStorageService, MetricsServiceTrait, VideoServiceTrait};

const THUMBNAIL_FILTER: &str =
    "scale=320:180:force_original_aspect_ratio=decrease,pad=320:180:(320-iw)/2:(180-ih)/2,setsar=1";

#[async_trait]
pub trait VideoProcessingServiceTrait: Send + Sync {
    async fn process_video(
        &self,
        video_id: Uuid,
        video_data: Vec<u8>,
        filename: &str,
        benchmark_run_id: Option<Uuid>,
    ) -> Result<()>;
}

pub struct VideoProcessingService {
    video_service: Arc<dyn VideoServiceTrait>,
    storage_service: Arc<dyn CloudStorageService>,
    metrics_service: Arc<dyn MetricsServiceTrait>,
}

impl VideoProcessingService {
    pub fn new(
        video_service: Arc<dyn VideoServiceTrait>,
        storage_service: Arc<dyn CloudStorageService>,
        metrics_service: Arc<dyn MetricsServiceTrait>,
    ) -> Self {
        Self {
            video_service,
            storage_service,
            metrics_service,
        }
    }

    async fn process_video_background(
        video_id: Uuid,
        local_input_path: String,
        temp_dir: String,
        storage_output_dir: String,
        storage_thumbnail_path: String,
        video_service: Arc<dyn VideoServiceTrait>,
        storage_service: Arc<dyn CloudStorageService>,
        metrics_service: Arc<dyn MetricsServiceTrait>,
        benchmark_run_id: Option<Uuid>,
    ) -> Result<()> {
        let local_output_dir = format!("{}/hls", temp_dir);
        let local_thumbnail_path = format!("{}/thumbnail.jpg", temp_dir);

        log::info!("process_video_background: Generating HLS streams in multiple resolutions");
        let hls_timer = Instant::now();
        let profile_count = Self::generate_hls_streams(&local_input_path, &local_output_dir)
            .await
            .context("Failed to generate HLS streams")?;
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "generate_hls_streams",
                Some(hls_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record generate_hls_streams metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video_background: Generating thumbnail");
        let thumbnail_timer = Instant::now();
        Self::generate_thumbnail(&local_input_path, &local_thumbnail_path)
            .await
            .context("Failed to generate thumbnail")?;
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "generate_thumbnail",
                Some(thumbnail_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record generate_thumbnail metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video_background: Uploading HLS files to cloud storage");
        let upload_hls_timer = Instant::now();
        let uploaded_files = Self::upload_hls_files_to_storage(
            &local_output_dir,
            &storage_output_dir,
            Arc::clone(&storage_service),
        )
        .await
        .context("Failed to upload HLS files to storage")?;
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "upload_hls_files",
                Some(upload_hls_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record upload_hls_files metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video_background: Uploading thumbnail to cloud storage");
        let upload_thumbnail_timer = Instant::now();
        let thumbnail_data = fs::read(&local_thumbnail_path)
            .await
            .context("Failed to read thumbnail file")?;
        storage_service
            .upload_file_data(thumbnail_data, &storage_thumbnail_path)
            .await
            .context("Failed to upload thumbnail to storage")?;
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "upload_thumbnail",
                Some(upload_thumbnail_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record upload_thumbnail metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video_background: Getting video duration");
        let duration_timer = Instant::now();
        let duration = Self::get_video_duration(&local_input_path)
            .await
            .context("Failed to get video duration")?;
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "get_video_duration",
                Some(duration_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record get_video_duration metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video_background: Updating video metadata");
        let metadata_timer = Instant::now();
        video_service
            .update_video_metadata(
                &video_id,
                Some(duration),
                Some(storage_thumbnail_path),
                Some(format!("{}playlist.m3u8", storage_output_dir)),
            )
            .await
            .context("Failed to update video metadata")?;
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "update_video_metadata",
                Some(metadata_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record update_video_metadata metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video_background: Updating video status to ready");
        let status_timer = Instant::now();
        video_service
            .update_video_status(&video_id, VideoStatus::Ready)
            .await
            .context("Failed to update video status to ready")?;
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "update_video_status_ready",
                Some(status_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record update_video_status_ready metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video_background: Cleaning up temporary files");
        let cleanup_timer = Instant::now();
        let cleanup_result = fs::remove_dir_all(&temp_dir).await;
        if let Err(e) = &cleanup_result {
            log::warn!("Failed to clean up temp directory {}: {}", temp_dir, e);
        }
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "cleanup_temp_dir",
                Some(cleanup_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record cleanup_temp_dir metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("Video processing completed successfully for {}", video_id);
        if let Err(err) = metrics_service
            .record_video_processing_step(
                benchmark_run_id,
                Some(video_id),
                "processing_complete",
                None,
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record processing_complete metric for {}: {}",
                video_id,
                err
            );
        }
        Ok(())
    }

    async fn generate_hls_streams(input_path: &str, output_dir: &str) -> Result<usize> {
        fs::create_dir_all(output_dir)
            .await
            .context("Failed to create output directory")?;

        let profiles = vec![
            ("1080p", 1920, 1080, "2000k", "192k"),
            ("720p", 1280, 720, "1000k", "128k"),
            ("480p", 854, 480, "500k", "96k"),
            ("360p", 640, 360, "250k", "64k"),
        ];

        let (source_width, source_height) = Self::get_video_dimensions(input_path)
            .await
            .context("Failed to get source video dimensions")?;

        let mut generated_profiles = Vec::with_capacity(profiles.len());

        let input_path_owned = input_path.to_owned();
        let output_dir_owned = output_dir.to_owned();

        let mut tasks = Vec::with_capacity(profiles.len());

        for &(quality, max_width, max_height, video_bitrate, audio_bitrate) in &profiles {
            let (target_width, target_height) = Self::calculate_scaled_dimensions(
                source_width,
                source_height,
                max_width,
                max_height,
            );

            let input_path_clone = input_path_owned.clone();
            let output_dir_clone = output_dir_owned.clone();

            let quality_label = quality.to_string();
            let quality_label_for_context = quality_label.clone();
            let quality_label_for_log = quality_label.clone();
            let video_bitrate_owned = video_bitrate.to_string();

            tasks.push(tokio::spawn(async move {
                let segment_filename = format!("{}/{}_%03d.ts", output_dir_clone, quality_label);
                let playlist_filename = format!("{}/{}.m3u8", output_dir_clone, quality_label);

                let scale_filter = format!("scale={}:{}", target_width, target_height);

                let output = Command::new("ffmpeg")
                    .args([
                        "-i",
                        &input_path_clone,
                        "-c:v",
                        "libx264",
                        "-c:a",
                        "aac",
                        "-b:v",
                        video_bitrate,
                        "-b:a",
                        audio_bitrate,
                        "-vf",
                        scale_filter.as_str(),
                        "-hls_time",
                        "10",
                        "-hls_playlist_type",
                        "vod",
                        "-hls_segment_filename",
                        &segment_filename,
                        "-f",
                        "hls",
                        &playlist_filename,
                        "-y",
                    ])
                    .output()
                    .await
                    .context(format!(
                        "Failed to execute FFmpeg command for {} quality",
                        quality_label_for_context
                    ))?;

                if !output.status.success() {
                    let error_msg = String::from_utf8_lossy(&output.stderr);
                    return Err(anyhow!(
                        "FFmpeg error for {} quality: {}",
                        quality_label_for_log,
                        error_msg
                    ));
                }

                log::info!("Generated {} quality stream", quality_label_for_log);
                Ok((
                    quality_label,
                    target_width,
                    target_height,
                    video_bitrate_owned,
                ))
            }));
        }

        for task in tasks {
            let profile = task
                .await
                .context("FFmpeg encoding task failed to join")??;
            generated_profiles.push(profile);
        }

        Self::generate_master_playlist(output_dir, &generated_profiles).await?;

        Ok(generated_profiles.len())
    }

    async fn generate_master_playlist(
        output_dir: &str,
        profiles: &[(String, i32, i32, String)],
    ) -> Result<()> {
        let mut master_playlist = String::from("#EXTM3U\n#EXT-X-VERSION:3\n\n");

        for (quality, width, height, video_bitrate) in profiles {
            let bandwidth = video_bitrate.replace('k', "000");
            master_playlist.push_str(&format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}x{}\n{}.m3u8\n\n",
                bandwidth, width, height, quality
            ));
        }

        fs::write(format!("{}/playlist.m3u8", output_dir), master_playlist)
            .await
            .context("Failed to write master playlist")?;

        log::info!("Generated master playlist");
        Ok(())
    }

    async fn get_video_dimensions(input_path: &str) -> Result<(i32, i32)> {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "csv=s=x:p=0",
                input_path,
            ])
            .output()
            .await
            .context("Failed to execute FFprobe command for dimensions")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("FFprobe error: {}", error_msg));
        }

        let dims = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let mut parts = dims.split('x');

        let width = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing width in FFprobe output"))?
            .parse::<i32>()
            .context("Failed to parse video width")?;
        let height = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing height in FFprobe output"))?
            .parse::<i32>()
            .context("Failed to parse video height")?;

        Ok((width, height))
    }

    fn calculate_scaled_dimensions(
        source_width: i32,
        source_height: i32,
        max_width: i32,
        max_height: i32,
    ) -> (i32, i32) {
        if source_width <= 0 || source_height <= 0 {
            return (max_width.max(2) & !1, max_height.max(2) & !1);
        }

        let width_ratio = max_width as f64 / source_width as f64;
        let height_ratio = max_height as f64 / source_height as f64;
        let scale_ratio = width_ratio.min(height_ratio).min(1.0);

        let mut target_width = (source_width as f64 * scale_ratio).round() as i32;
        let mut target_height = (source_height as f64 * scale_ratio).round() as i32;

        target_width = target_width.max(2);
        target_height = target_height.max(2);

        if target_width % 2 != 0 {
            target_width -= 1;
        }
        if target_height % 2 != 0 {
            target_height -= 1;
        }

        target_width = target_width.max(2);
        target_height = target_height.max(2);

        (target_width, target_height)
    }

    // async fn generate_thumbnail_from_url(input_url: &str, thumbnail_path: &str) -> Result<()> {
    //     if let Some(parent) = Path::new(thumbnail_path).parent() {
    //         fs::create_dir_all(parent)
    //             .await
    //             .context("Failed to create thumbnail directory")?;
    //     }

    //     let output = Command::new("ffmpeg")
    //         .args([
    //             "-i",
    //             input_url,
    //             "-ss",
    //             "00:00:01",
    //             "-vframes",
    //             "1",
    //             "-q:v",
    //             "2",
    //             "-vf",
    //             THUMBNAIL_FILTER,
    //             "-y",
    //             thumbnail_path,
    //         ])
    //         .output()
    //         .context("Failed to execute FFmpeg thumbnail command")?;

    //     if !output.status.success() {
    //         let error_msg = String::from_utf8_lossy(&output.stderr);
    //         return Err(anyhow::anyhow!("FFmpeg thumbnail error: {}", error_msg));
    //     }

    //     log::info!("Generated thumbnail from URL: {}", thumbnail_path);
    //     Ok(())
    // }

    async fn generate_thumbnail(input_path: &str, thumbnail_path: &str) -> Result<()> {
        if let Some(parent) = Path::new(thumbnail_path).parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create thumbnail directory")?;
        }

        let output = Command::new("ffmpeg")
            .args([
                "-i",
                input_path,
                "-ss",
                "00:00:01",
                "-vframes",
                "1",
                "-q:v",
                "2",
                "-vf",
                THUMBNAIL_FILTER,
                "-y",
                thumbnail_path,
            ])
            .output()
            .await
            .context("Failed to execute FFmpeg thumbnail command")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("FFmpeg thumbnail error: {}", error_msg));
        }

        log::info!("Generated thumbnail: {}", thumbnail_path);
        Ok(())
    }

    async fn get_video_duration(input_path: &str) -> Result<i32> {
        let output = Command::new("ffprobe")
            .args([
                "-v",
                "quiet",
                "-show_entries",
                "format=duration",
                "-of",
                "csv=p=0",
                input_path,
            ])
            .output()
            .await
            .context("Failed to execute FFprobe command")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("FFprobe error: {}", error_msg));
        }

        let duration_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let duration: f64 = duration_str
            .parse()
            .context("Failed to parse video duration")?;

        Ok(duration as i32)
    }

    async fn upload_hls_files_to_storage(
        local_output_dir: &str,
        storage_output_dir: &str,
        storage_service: Arc<dyn CloudStorageService>,
    ) -> Result<usize> {
        let mut entries = fs::read_dir(local_output_dir)
            .await
            .context("Failed to read local output directory")?;

        let mut uploaded_files = 0usize;

        while let Some(entry) = entries.next_entry().await? {
            let local_path = entry.path();
            if local_path.is_file() {
                let filename = local_path
                    .file_name()
                    .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?
                    .to_string_lossy();

                let storage_path = format!("{}{}", storage_output_dir, filename);

                let file_data = fs::read(&local_path)
                    .await
                    .context(format!("Failed to read {} file", filename))?;
                storage_service
                    .upload_file_data(file_data, &storage_path)
                    .await
                    .context(format!("Failed to upload {} to storage", filename))?;

                log::info!("Uploaded {} to cloud storage", filename);
                uploaded_files += 1;
            }
        }

        Ok(uploaded_files)
    }

    pub fn validate_video_file(filename: &str, file_size: u64) -> Result<()> {
        let extension = filename.split('.').last().unwrap_or("").to_lowercase();
        let supported_formats = ["mp4", "mov", "avi", "mkv", "webm", "flv", "wmv", "m4v"];

        if !supported_formats.contains(&extension.as_str()) {
            return Err(anyhow::anyhow!(
                "Unsupported video format: {}. Supported formats: {}",
                extension,
                supported_formats.join(", ")
            ));
        }

        const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;
        if file_size > MAX_FILE_SIZE {
            return Err(anyhow::anyhow!(
                "File size too large: {} MB. Maximum allowed: {} MB",
                file_size / 1024 / 1024,
                MAX_FILE_SIZE / 1024 / 1024
            ));
        }

        const MIN_FILE_SIZE: u64 = 1024 * 1024;
        if file_size < MIN_FILE_SIZE {
            return Err(anyhow::anyhow!(
                "File size too small: {} bytes. Minimum required: {} bytes",
                file_size,
                MIN_FILE_SIZE
            ));
        }

        Ok(())
    }

    pub fn check_ffmpeg_availability() -> Result<()> {
        let output = std::process::Command::new("ffmpeg")
            .args(["-version"])
            .output()
            .context("Failed to execute FFmpeg command")?;

        if !output.status.success() {
            return Err(anyhow::anyhow!(
                "FFmpeg is not installed or not available in PATH"
            ));
        }

        Ok(())
    }
}

#[async_trait]
impl VideoProcessingServiceTrait for VideoProcessingService {
    async fn process_video(
        &self,
        video_id: Uuid,
        video_data: Vec<u8>,
        filename: &str,
        benchmark_run_id: Option<Uuid>,
    ) -> Result<()> {
        self.video_service
            .update_video_status(&video_id, VideoStatus::Processing)
            .await
            .context("Failed to update video status to processing")?;

        let output_dir = self.storage_service.get_hls_path(&video_id);
        let thumbnail_path = self.storage_service.get_thumbnail_path(&video_id);

        let mut processing_run_id = benchmark_run_id;
        if processing_run_id.is_none() {
            let metadata = json!({
                "video_id": video_id,
                "filename": filename,
                "payload_bytes": video_data.len(),
            });
            match self
                .metrics_service
                .create_benchmark_run("video_processing", Some(metadata))
                .await
            {
                Ok(run_id) => processing_run_id = Some(run_id),
                Err(err) => log::warn!("Unable to create benchmark run for {}: {}", video_id, err),
            }
        }

        log::info!("process_video: Creating temp directory for processing");
        let temp_dir = format!("/tmp/video_processing/{}", video_id);
        let temp_dir_timer = Instant::now();
        fs::create_dir_all(&temp_dir)
            .await
            .context("Failed to create temp directory")?;
        if let Err(err) = self
            .metrics_service
            .record_video_processing_step(
                processing_run_id,
                Some(video_id),
                "create_temp_dir",
                Some(temp_dir_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record create_temp_dir metric for {}: {}",
                video_id,
                err
            );
        }

        log::info!("process_video: Created temp directory: {}", temp_dir);

        let local_input_path = format!("{}/{}", temp_dir, filename);
        let write_timer = Instant::now();
        fs::write(&local_input_path, &video_data)
            .await
            .context("Failed to write video data to temp file")?;
        if let Err(err) = self
            .metrics_service
            .record_video_processing_step(
                processing_run_id,
                Some(video_id),
                "write_input_file",
                Some(write_timer.elapsed().as_millis() as i64),
                None,
                None,
            )
            .await
        {
            log::warn!(
                "Failed to record write_input_file metric for {}: {}",
                video_id,
                err
            );
        }

        let video_service_clone = Arc::clone(&self.video_service);
        let storage_service_clone = Arc::clone(&self.storage_service);
        let metrics_service_clone = Arc::clone(&self.metrics_service);
        let processing_run_id_clone = processing_run_id;

        tokio::spawn(async move {
            let processing_video_service = Arc::clone(&video_service_clone);
            let processing_storage_service = Arc::clone(&storage_service_clone);
            let processing_metrics_service = Arc::clone(&metrics_service_clone);
            match VideoProcessingService::process_video_background(
                video_id,
                local_input_path,
                temp_dir,
                output_dir,
                thumbnail_path,
                processing_video_service,
                processing_storage_service,
                processing_metrics_service.clone(),
                processing_run_id_clone,
            )
            .await
            {
                Ok(_) => {
                    log::info!(
                        "✅ Video processing completed successfully for {}",
                        video_id
                    );
                }
                Err(e) => {
                    log::error!("❌ Video processing failed for {}: {}", video_id, e);
                    log::error!("🔍 Error details: {:?}", e);
                    if let Err(err) = processing_metrics_service
                        .record_video_processing_step(
                            processing_run_id_clone,
                            Some(video_id),
                            "processing_failed",
                            None,
                            None,
                            None,
                        )
                        .await
                    {
                        log::warn!(
                            "Failed to record processing_failed metric for {}: {}",
                            video_id,
                            err
                        );
                    }
                    if let Err(update_err) = video_service_clone
                        .update_video_status(&video_id, VideoStatus::Failed)
                        .await
                    {
                        log::error!("❌ Failed to update video status to failed: {}", update_err);
                    }
                }
            }
        });

        Ok(())
    }
}
