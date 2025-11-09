use anyhow::{Context, Result};
use async_trait::async_trait;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tokio::fs;
use tokio::task;
use uuid::Uuid;

use crate::models::VideoStatus;
use crate::services::{CloudStorageService, VideoServiceTrait};

#[async_trait]
pub trait VideoProcessingServiceTrait: Send + Sync {
    async fn process_video(
        &self,
        video_id: Uuid,
        video_data: Vec<u8>,
        filename: &str,
    ) -> Result<()>;
}

pub struct VideoProcessingService {
    video_service: Arc<dyn VideoServiceTrait>,
    storage_service: Arc<dyn CloudStorageService>,
}

impl VideoProcessingService {
    pub fn new(
        video_service: Arc<dyn VideoServiceTrait>,
        storage_service: Arc<dyn CloudStorageService>,
    ) -> Self {
        Self {
            video_service,
            storage_service,
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
    ) -> Result<()> {
        let local_output_dir = format!("{}/hls", temp_dir);
        let local_thumbnail_path = format!("{}/thumbnail.jpg", temp_dir);

        log::info!("process_video_background: Generating HLS streams in multiple resolutions");
        Self::generate_hls_streams(&local_input_path, &local_output_dir)
            .await
            .context("Failed to generate HLS streams")?;

        log::info!("process_video_background: Generating thumbnail");
        Self::generate_thumbnail(&local_input_path, &local_thumbnail_path)
            .await
            .context("Failed to generate thumbnail")?;

        log::info!("process_video_background: Uploading HLS files to cloud storage");
        Self::upload_hls_files_to_storage(
            &local_output_dir,
            &storage_output_dir,
            Arc::clone(&storage_service),
        )
        .await
        .context("Failed to upload HLS files to storage")?;

        log::info!("process_video_background: Uploading thumbnail to cloud storage");
        let thumbnail_data = fs::read(&local_thumbnail_path)
            .await
            .context("Failed to read thumbnail file")?;
        storage_service
            .upload_file_data(thumbnail_data, &storage_thumbnail_path)
            .await
            .context("Failed to upload thumbnail to storage")?;

        log::info!("process_video_background: Getting video duration");
        let duration = Self::get_video_duration(&local_input_path)
            .await
            .context("Failed to get video duration")?;

        log::info!("process_video_background: Updating video metadata");
        video_service
            .update_video_metadata(
                &video_id,
                Some(duration),
                Some(storage_thumbnail_path),
                Some(format!("{}playlist.m3u8", storage_output_dir)),
            )
            .await
            .context("Failed to update video metadata")?;

        log::info!("process_video_background: Updating video status to ready");
        video_service
            .update_video_status(&video_id, VideoStatus::Ready)
            .await
            .context("Failed to update video status to ready")?;

        log::info!("process_video_background: Cleaning up temporary files");
        if let Err(e) = fs::remove_dir_all(&temp_dir).await {
            log::warn!("Failed to clean up temp directory {}: {}", temp_dir, e);
        }

        log::info!("Video processing completed successfully for {}", video_id);
        Ok(())
    }

    // async fn generate_hls_streams_from_url(input_url: &str, output_dir: &str) -> Result<()> {
    //     fs::create_dir_all(output_dir)
    //         .await
    //         .context("Failed to create output directory")?;

    //     let profiles = vec![
    //         ("1080p", "1920x1080", "2000k", "192k"),
    //         ("720p", "1280x720", "1000k", "128k"),
    //         ("480p", "854x480", "500k", "96k"),
    //         ("360p", "640x360", "250k", "64k"),
    //     ];

    //     for (quality, resolution, video_bitrate, audio_bitrate) in profiles.clone() {
    //         let segment_filename = format!("{}/{}_%03d.ts", output_dir, quality);
    //         let playlist_filename = format!("{}/{}.m3u8", output_dir, quality);

    //         let output = Command::new("ffmpeg")
    //             .args([
    //                 "-i",
    //                 input_url,
    //                 "-c:v",
    //                 "libx264",
    //                 "-c:a",
    //                 "aac",
    //                 "-b:v",
    //                 video_bitrate,
    //                 "-b:a",
    //                 audio_bitrate,
    //                 "-vf",
    //                 &format!("scale={}", resolution),
    //                 "-hls_time",
    //                 "10",
    //                 "-hls_playlist_type",
    //                 "vod",
    //                 "-hls_segment_filename",
    //                 &segment_filename,
    //                 "-f",
    //                 "hls",
    //                 &playlist_filename,
    //                 "-y",
    //             ])
    //             .output()
    //             .context("Failed to execute FFmpeg command")?;

    //         if !output.status.success() {
    //             let error_msg = String::from_utf8_lossy(&output.stderr);
    //             return Err(anyhow::anyhow!(
    //                 "FFmpeg error for {} quality: {}",
    //                 quality,
    //                 error_msg
    //             ));
    //         }

    //         log::info!("Generated {} quality stream from URL", quality);
    //     }

    //     Self::generate_master_playlist(output_dir, &profiles).await?;

    //     Ok(())
    // }

    async fn generate_hls_streams(input_path: &str, output_dir: &str) -> Result<()> {
        fs::create_dir_all(output_dir)
            .await
            .context("Failed to create output directory")?;

        let profiles = vec![
            ("1080p", "1920x1080", "2000k", "192k"),
            ("720p", "1280x720", "1000k", "128k"),
            ("480p", "854x480", "500k", "96k"),
            ("360p", "640x360", "250k", "64k"),
        ];

        for (quality, resolution, video_bitrate, audio_bitrate) in profiles.clone() {
            let segment_filename = format!("{}/{}_%03d.ts", output_dir, quality);
            let playlist_filename = format!("{}/{}.m3u8", output_dir, quality);

            let output = Command::new("ffmpeg")
                .args([
                    "-i",
                    input_path,
                    "-c:v",
                    "libx264",
                    "-c:a",
                    "aac",
                    "-b:v",
                    video_bitrate,
                    "-b:a",
                    audio_bitrate,
                    "-vf",
                    &format!("scale={}", resolution),
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
                .context("Failed to execute FFmpeg command")?;

            if !output.status.success() {
                let error_msg = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "FFmpeg error for {} quality: {}",
                    quality,
                    error_msg
                ));
            }

            log::info!("Generated {} quality stream", quality);
        }

        Self::generate_master_playlist(output_dir, &profiles).await?;

        Ok(())
    }

    async fn generate_master_playlist(
        output_dir: &str,
        profiles: &[(&str, &str, &str, &str)],
    ) -> Result<()> {
        let mut master_playlist = String::from("#EXTM3U\n#EXT-X-VERSION:3\n\n");

        for (quality, resolution, video_bitrate, _audio_bitrate) in profiles {
            let bandwidth = video_bitrate.replace('k', "000");
            master_playlist.push_str(&format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}\n{}.m3u8\n\n",
                bandwidth, resolution, quality
            ));
        }

        fs::write(format!("{}/playlist.m3u8", output_dir), master_playlist)
            .await
            .context("Failed to write master playlist")?;

        log::info!("Generated master playlist");
        Ok(())
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
    //             "scale=320:180",
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
                "scale=320:180",
                "-y",
                thumbnail_path,
            ])
            .output()
            .context("Failed to execute FFmpeg thumbnail command")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!("FFmpeg thumbnail error: {}", error_msg));
        }

        log::info!("Generated thumbnail: {}", thumbnail_path);
        Ok(())
    }

    // async fn get_video_duration_from_url(input_url: &str) -> Result<i32> {
    //     let output = Command::new("ffprobe")
    //         .args([
    //             "-v",
    //             "quiet",
    //             "-show_entries",
    //             "format=duration",
    //             "-of",
    //             "csv=p=0",
    //             input_url,
    //         ])
    //         .output()
    //         .context("Failed to execute FFprobe command")?;

    //     if !output.status.success() {
    //         let error_msg = String::from_utf8_lossy(&output.stderr);
    //         return Err(anyhow::anyhow!("FFprobe error: {}", error_msg));
    //     }

    //     let duration_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    //     let duration: f64 = duration_str
    //         .parse()
    //         .context("Failed to parse video duration")?;

    //     Ok(duration as i32)
    // }

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
    ) -> Result<()> {
        let mut entries = fs::read_dir(local_output_dir)
            .await
            .context("Failed to read local output directory")?;

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
            }
        }

        Ok(())
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
        let output = Command::new("ffmpeg")
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
    ) -> Result<()> {
        self.video_service
            .update_video_status(&video_id, VideoStatus::Processing)
            .await
            .context("Failed to update video status to processing")?;

        let output_dir = self.storage_service.get_hls_path(&video_id);
        let thumbnail_path = self.storage_service.get_thumbnail_path(&video_id);

        log::info!("process_video: Creating temp directory for processing");
        let temp_dir = format!("/tmp/video_processing/{}", video_id);
        fs::create_dir_all(&temp_dir)
            .await
            .context("Failed to create temp directory")?;

        log::info!("process_video: Created temp directory: {}", temp_dir);

        let local_input_path = format!("{}/{}", temp_dir, filename);
        fs::write(&local_input_path, &video_data)
            .await
            .context("Failed to write video data to temp file")?;

        let video_service_clone = Arc::clone(&self.video_service);
        let storage_service_clone = Arc::clone(&self.storage_service);

        tokio::spawn(async move {
            log::info!("🎬 Starting background video processing for {}", video_id);
            let processing_video_service = Arc::clone(&video_service_clone);
            let processing_storage_service = Arc::clone(&storage_service_clone);
            match VideoProcessingService::process_video_background(
                video_id,
                local_input_path,
                temp_dir,
                output_dir,
                thumbnail_path,
                processing_video_service,
                processing_storage_service,
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
