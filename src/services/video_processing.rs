use anyhow::{Result, Context};
use std::process::Command;
use std::path::Path;
use uuid::Uuid;
use tokio::fs;
use tokio::task;
use crate::services::{
    VideoService, 
    GcsService
};
use crate::models::VideoStatus;

pub struct VideoProcessingService {
    video_service: VideoService,
    gcs_service: GcsService,
}

impl VideoProcessingService {
    pub fn new(
        video_service: VideoService, 
        gcs_service: GcsService
    ) -> Self {
        Self {
            video_service,
            gcs_service,
        }
    }

    /// Process video file: convert to HLS, generate thumbnail, update database
    pub async fn process_video(&self, video_id: Uuid, video_data: Vec<u8>, filename: &str) -> Result<()> {
        // Update status to processing
        self.video_service.update_video_status(&video_id, VideoStatus::Processing).await
            .context("Failed to update video status to processing")?;

        let output_dir = self.gcs_service.get_hls_path(&video_id);
        let thumbnail_path = self.gcs_service.get_thumbnail_path(&video_id);

        // Create temp directory for processing
        log::info!("process_video: Creating temp directory for processing");
        let temp_dir = format!("/tmp/video_processing/{}", video_id);
        fs::create_dir_all(&temp_dir).await
            .context("Failed to create temp directory")?;
        
        log::info!("process_video: Created temp directory: {}", temp_dir);

        // Save video data to temporary file for processing
        let local_input_path = format!("{}/{}", temp_dir, filename);
        fs::write(&local_input_path, &video_data).await
            .context("Failed to write video data to temp file")?;

        // Process video in background task
        let video_service_clone = self.video_service.clone();
        let gcs_service_clone = self.gcs_service.clone();
        
        task::spawn(async move {
            if let Err(e) = Self::process_video_background(
                video_id,
                local_input_path,
                temp_dir,
                output_dir,
                thumbnail_path,
                video_service_clone.clone(),
                gcs_service_clone,
            ).await {
                log::error!("Video processing failed for {}: {}", video_id, e);
                let _ = video_service_clone.update_video_status(&video_id, VideoStatus::Failed).await;
            }
        });

        Ok(())
    }

    /// Background processing function
    async fn process_video_background(
        video_id: Uuid,
        local_input_path: String,
        temp_dir: String,
        gcs_output_dir: String,
        gcs_thumbnail_path: String,
        video_service: VideoService,
        gcs_service: GcsService,
    ) -> Result<()> {
        // Create local output directory for processing
        let local_output_dir = format!("{}/hls", temp_dir);
        let local_thumbnail_path = format!("{}/thumbnail.jpg", temp_dir);
        
        // Generate HLS streams in multiple resolutions
        log::info!("process_video_background: Generating HLS streams in multiple resolutions");
        Self::generate_hls_streams(&local_input_path, &local_output_dir).await
            .context("Failed to generate HLS streams")?;

        // Generate thumbnail
        log::info!("process_video_background: Generating thumbnail");
        Self::generate_thumbnail(&local_input_path, &local_thumbnail_path).await
            .context("Failed to generate thumbnail")?;

        // Upload HLS files to GCS
        log::info!("process_video_background: Uploading HLS files to GCS");
        Self::upload_hls_files_to_gcs(&local_output_dir, &gcs_output_dir, &gcs_service).await
            .context("Failed to upload HLS files to GCS")?;

        // Upload thumbnail to GCS
        log::info!("process_video_background: Uploading thumbnail to GCS");
        let thumbnail_data = fs::read(&local_thumbnail_path).await
            .context("Failed to read thumbnail file")?;
        gcs_service.upload_file_data(thumbnail_data, &gcs_thumbnail_path).await
            .context("Failed to upload thumbnail to GCS")?;

        // Get video duration
        log::info!("process_video_background: Getting video duration");
        let duration = Self::get_video_duration(&local_input_path).await
            .context("Failed to get video duration")?;

        // Update video metadata
        log::info!("process_video_background: Updating video metadata");
        video_service.update_video_metadata(
            &video_id,
            Some(duration),
            Some(gcs_thumbnail_path),
            Some(format!("{}playlist.m3u8", gcs_output_dir)),
        ).await.context("Failed to update video metadata")?;

        // Update status to ready
        log::info!("process_video_background: Updating video status to ready");
        video_service.update_video_status(&video_id, VideoStatus::Ready).await
            .context("Failed to update video status to ready")?;

        // Clean up temporary files
        log::info!("process_video_background: Cleaning up temporary files");
        if let Err(e) = fs::remove_dir_all(&temp_dir).await {
            log::warn!("Failed to clean up temp directory {}: {}", temp_dir, e);
        }

        log::info!("Video processing completed successfully for {}", video_id);
        Ok(())
    }

    // pub async fn process_video(&self, video_id: Uuid) -> Result<()> {
    //     // Update status to processing
    //     self.video_service.update_video_status(&video_id, VideoStatus::Processing).await?;

    //     // Get video info
    //     let video = self.video_service.get_video_by_id(&video_id).await?
    //         .ok_or_else(|| anyhow::anyhow!("Video not found"))?;

    //     let input_path = self.storage_service.get_video_path(&video_id, &video.filename);
    //     let output_dir = self.storage_service.get_hls_path(&video_id);
    //     let thumbnail_path = self.storage_service.get_thumbnail_path(&video_id);

    //     // Create output directory
    //     self.storage_service.create_video_directory(&video_id).await?;

    //     // Generate HLS streams in multiple resolutions
    //     self.generate_hls_streams(&input_path, &output_dir).await?;

    //     // Generate thumbnail
    //     self.generate_thumbnail(&input_path, &thumbnail_path).await?;

    //     // Get video duration
    //     let duration = self.get_video_duration(&input_path).await?;

    //     // Update video metadata
    //     self.video_service.update_video_metadata(
    //         &video_id,
    //         Some(duration),
    //         Some(format!("thumbnails/{}.jpg", video_id)),
    //         Some(format!("hls/{}/playlist.m3u8", video_id)),
    //     ).await?;

    //     // Update status to ready
    //     self.video_service.update_video_status(&video_id, VideoStatus::Ready).await?;

    //     Ok(())
    // }

    /// Generate HLS streams with multiple bitrates using FFmpeg from URL
    async fn generate_hls_streams_from_url(input_url: &str, output_dir: &str) -> Result<()> {
        // Ensure output directory exists
        fs::create_dir_all(output_dir).await
            .context("Failed to create output directory")?;

        // Define quality profiles
        let profiles = vec![
            ("1080p", "1920x1080", "2000k", "192k"),
            ("720p", "1280x720", "1000k", "128k"),
            ("480p", "854x480", "500k", "96k"),
            ("360p", "640x360", "250k", "64k"),
        ];

        // Generate individual quality streams
        for (quality, resolution, video_bitrate, audio_bitrate) in profiles.clone() {
            let segment_filename = format!("{}/{}_%03d.ts", output_dir, quality);
            let playlist_filename = format!("{}/{}.m3u8", output_dir, quality);

            let output = Command::new("ffmpeg")
                .args([
                    "-i", input_url,
                    "-c:v", "libx264",
                    "-c:a", "aac",
                    "-b:v", video_bitrate,
                    "-b:a", audio_bitrate,
                    "-vf", &format!("scale={}", resolution),
                    "-hls_time", "10",
                    "-hls_playlist_type", "vod",
                    "-hls_segment_filename", &segment_filename,
                    "-f", "hls",
                    &playlist_filename,
                    "-y", // Overwrite output files
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

            log::info!("Generated {} quality stream from URL", quality);
        }

        // Generate master playlist
        Self::generate_master_playlist(output_dir, &profiles).await?;

        Ok(())
    }

    /// Generate HLS streams with multiple bitrates using FFmpeg
    async fn generate_hls_streams(input_path: &str, output_dir: &str) -> Result<()> {
        // Ensure output directory exists
        fs::create_dir_all(output_dir).await
            .context("Failed to create output directory")?;

        // Define quality profiles
        let profiles = vec![
            ("1080p", "1920x1080", "2000k", "192k"),
            ("720p", "1280x720", "1000k", "128k"),
            ("480p", "854x480", "500k", "96k"),
            ("360p", "640x360", "250k", "64k"),
        ];

        // Generate individual quality streams
        for (quality, resolution, video_bitrate, audio_bitrate) in profiles.clone() {
            let segment_filename = format!("{}/{}_%03d.ts", output_dir, quality);
            let playlist_filename = format!("{}/{}.m3u8", output_dir, quality);

            let output = Command::new("ffmpeg")
                .args([
                    "-i", input_path,
                    "-c:v", "libx264",
                    "-c:a", "aac",
                    "-b:v", video_bitrate,
                    "-b:a", audio_bitrate,
                    "-vf", &format!("scale={}", resolution),
                    "-hls_time", "10",
                    "-hls_playlist_type", "vod",
                    "-hls_segment_filename", &segment_filename,
                    "-f", "hls",
                    &playlist_filename,
                    "-y", // Overwrite output files
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

        // Generate master playlist
        Self::generate_master_playlist(output_dir, &profiles).await?;

        Ok(())
    }

    /// Generate master playlist for adaptive streaming
    async fn generate_master_playlist(output_dir: &str, profiles: &[(&str, &str, &str, &str)]) -> Result<()> {
        let mut master_playlist = String::from("#EXTM3U\n#EXT-X-VERSION:3\n\n");

        // Add stream entries for each quality
        for (quality, resolution, video_bitrate, _audio_bitrate) in profiles {
            let bandwidth = video_bitrate.replace('k', "000");
            master_playlist.push_str(&format!(
                "#EXT-X-STREAM-INF:BANDWIDTH={},RESOLUTION={}\n{}.m3u8\n\n",
                bandwidth, resolution, quality
            ));
        }

        fs::write(format!("{}/playlist.m3u8", output_dir), master_playlist).await
            .context("Failed to write master playlist")?;

        log::info!("Generated master playlist");
        Ok(())
    }

    /// Generate thumbnail from video using FFmpeg from URL
    async fn generate_thumbnail_from_url(input_url: &str, thumbnail_path: &str) -> Result<()> {
        // Ensure thumbnail directory exists
        if let Some(parent) = Path::new(thumbnail_path).parent() {
            fs::create_dir_all(parent).await
                .context("Failed to create thumbnail directory")?;
        }

        let output = Command::new("ffmpeg")
            .args([
                "-i", input_url,
                "-ss", "00:00:01", // Seek to 1 second
                "-vframes", "1",   // Extract 1 frame
                "-q:v", "2",       // High quality
                "-vf", "scale=320:180", // Resize to standard thumbnail size
                "-y",              // Overwrite output file
                thumbnail_path,
            ])
            .output()
            .context("Failed to execute FFmpeg thumbnail command")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "FFmpeg thumbnail error: {}", 
                error_msg
            ));
        }

        log::info!("Generated thumbnail from URL: {}", thumbnail_path);
        Ok(())
    }

    /// Generate thumbnail from video using FFmpeg
    async fn generate_thumbnail(input_path: &str, thumbnail_path: &str) -> Result<()> {
        // Ensure thumbnail directory exists
        if let Some(parent) = Path::new(thumbnail_path).parent() {
            fs::create_dir_all(parent).await
                .context("Failed to create thumbnail directory")?;
        }

        let output = Command::new("ffmpeg")
            .args([
                "-i", input_path,
                "-ss", "00:00:01", // Seek to 1 second
                "-vframes", "1",   // Extract 1 frame
                "-q:v", "2",       // High quality
                "-vf", "scale=320:180", // Resize to standard thumbnail size
                "-y",              // Overwrite output file
                thumbnail_path,
            ])
            .output()
            .context("Failed to execute FFmpeg thumbnail command")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "FFmpeg thumbnail error: {}", 
                error_msg
            ));
        }

        log::info!("Generated thumbnail: {}", thumbnail_path);
        Ok(())
    }

    /// Get video duration using FFprobe from URL
    async fn get_video_duration_from_url(input_url: &str) -> Result<i32> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-show_entries", "format=duration",
                "-of", "csv=p=0",
                input_url,
            ])
            .output()
            .context("Failed to execute FFprobe command")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "FFprobe error: {}", 
                error_msg
            ));
        }

        let duration_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let duration: f64 = duration_str.parse()
            .context("Failed to parse video duration")?;
        
        Ok(duration as i32)
    }

    /// Get video duration using FFprobe
    async fn get_video_duration(input_path: &str) -> Result<i32> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-show_entries", "format=duration",
                "-of", "csv=p=0",
                input_path,
            ])
            .output()
            .context("Failed to execute FFprobe command")?;

        if !output.status.success() {
            let error_msg = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow::anyhow!(
                "FFprobe error: {}", 
                error_msg
            ));
        }

        let duration_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let duration: f64 = duration_str.parse()
            .context("Failed to parse video duration")?;
        
        Ok(duration as i32)
    }

    /// Upload HLS files to GCS
    async fn upload_hls_files_to_gcs(
        local_output_dir: &str,
        gcs_output_dir: &str,
        gcs_service: &GcsService,
    ) -> Result<()> {
        let mut entries = fs::read_dir(local_output_dir).await
            .context("Failed to read local output directory")?;

        while let Some(entry) = entries.next_entry().await? {
            let local_path = entry.path();
            if local_path.is_file() {
                let filename = local_path.file_name()
                    .ok_or_else(|| anyhow::anyhow!("Invalid filename"))?
                    .to_string_lossy();
                
                let gcs_path = format!("{}{}", gcs_output_dir, filename);
                
                let file_data = fs::read(&local_path).await
                    .context(format!("Failed to read {} file", filename))?;
                gcs_service.upload_file_data(file_data, &gcs_path).await
                    .context(format!("Failed to upload {} to GCS", filename))?;
                
                log::info!("Uploaded {} to GCS", filename);
            }
        }

        Ok(())
    }

    /// Validate video file format and size
    pub fn validate_video_file(filename: &str, file_size: u64) -> Result<()> {
        // Check file extension
        let extension = filename.split('.').last().unwrap_or("").to_lowercase();
        let supported_formats = ["mp4", "mov", "avi", "mkv", "webm", "flv", "wmv", "m4v"];
        
        if !supported_formats.contains(&extension.as_str()) {
            return Err(anyhow::anyhow!(
                "Unsupported video format: {}. Supported formats: {}", 
                extension, 
                supported_formats.join(", ")
            ));
        }

        // Check file size (2GB limit)
        const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024; // 2GB
        if file_size > MAX_FILE_SIZE {
            return Err(anyhow::anyhow!(
                "File size too large: {} MB. Maximum allowed: {} MB", 
                file_size / 1024 / 1024,
                MAX_FILE_SIZE / 1024 / 1024
            ));
        }

        // Check minimum file size (1MB)
        const MIN_FILE_SIZE: u64 = 1024 * 1024; // 1MB
        if file_size < MIN_FILE_SIZE {
            return Err(anyhow::anyhow!(
                "File size too small: {} bytes. Minimum required: {} bytes", 
                file_size,
                MIN_FILE_SIZE
            ));
        }

        Ok(())
    }

    /// Check if FFmpeg is available on the system
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
