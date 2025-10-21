use anyhow::Result;
use std::process::Command;
use uuid::Uuid;
use crate::services::{
    // VideoService, 
    StorageService
};
use crate::models::VideoStatus;

pub struct VideoProcessingService {
    // video_service: VideoService,
    storage_service: StorageService,
}

impl VideoProcessingService {
    pub fn new(
        // video_service: VideoService, 
        storage_service: StorageService
    ) -> Self {
        Self {
            // video_service,
            storage_service,
        }
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

    async fn generate_hls_streams(&self, input_path: &str, output_dir: &str) -> Result<()> {
        let playlist_path = format!("{}/playlist.m3u8", output_dir);

        // FFmpeg command to generate HLS streams in multiple resolutions
        let output = Command::new("ffmpeg")
            .args([
                "-i", input_path,
                "-c:v", "libx264",
                "-c:a", "aac",
                "-b:v", "1000k",
                "-b:a", "128k",
                "-vf", "scale=1280:720",
                "-hls_time", "10",
                "-hls_playlist_type", "vod",
                "-hls_segment_filename", &format!("{}/720p_%03d.ts", output_dir),
                "-f", "hls",
                &format!("{}/720p.m3u8", output_dir),
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("FFmpeg error: {}", String::from_utf8_lossy(&output.stderr)));
        }

        // Generate 480p stream
        let output = Command::new("ffmpeg")
            .args([
                "-i", input_path,
                "-c:v", "libx264",
                "-c:a", "aac",
                "-b:v", "500k",
                "-b:a", "96k",
                "-vf", "scale=854:480",
                "-hls_time", "10",
                "-hls_playlist_type", "vod",
                "-hls_segment_filename", &format!("{}/480p_%03d.ts", output_dir),
                "-f", "hls",
                &format!("{}/480p.m3u8", output_dir),
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("FFmpeg error: {}", String::from_utf8_lossy(&output.stderr)));
        }

        // Generate 360p stream
        let output = Command::new("ffmpeg")
            .args([
                "-i", input_path,
                "-c:v", "libx264",
                "-c:a", "aac",
                "-b:v", "250k",
                "-b:a", "64k",
                "-vf", "scale=640:360",
                "-hls_time", "10",
                "-hls_playlist_type", "vod",
                "-hls_segment_filename", &format!("{}/360p_%03d.ts", output_dir),
                "-f", "hls",
                &format!("{}/360p.m3u8", output_dir),
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("FFmpeg error: {}", String::from_utf8_lossy(&output.stderr)));
        }

        // Generate master playlist
        self.generate_master_playlist(output_dir).await?;

        Ok(())
    }

    async fn generate_master_playlist(&self, output_dir: &str) -> Result<()> {
        let master_playlist = format!(
            "#EXTM3U
#EXT-X-VERSION:3

#EXT-X-STREAM-INF:BANDWIDTH=1000000,RESOLUTION=1280x720
720p.m3u8

#EXT-X-STREAM-INF:BANDWIDTH=500000,RESOLUTION=854x480
480p.m3u8

#EXT-X-STREAM-INF:BANDWIDTH=250000,RESOLUTION=640x360
360p.m3u8
"
        );

        tokio::fs::write(format!("{}/playlist.m3u8", output_dir), master_playlist).await?;
        Ok(())
    }

    async fn generate_thumbnail(&self, input_path: &str, thumbnail_path: &str) -> Result<()> {
        let output = Command::new("ffmpeg")
            .args([
                "-i", input_path,
                "-ss", "00:00:01",
                "-vframes", "1",
                "-q:v", "2",
                "-y",
                thumbnail_path,
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("FFmpeg thumbnail error: {}", String::from_utf8_lossy(&output.stderr)));
        }

        Ok(())
    }

    async fn get_video_duration(&self, input_path: &str) -> Result<i32> {
        let output = Command::new("ffprobe")
            .args([
                "-v", "quiet",
                "-show_entries", "format=duration",
                "-of", "csv=p=0",
                input_path,
            ])
            .output()?;

        if !output.status.success() {
            return Err(anyhow::anyhow!("FFprobe error: {}", String::from_utf8_lossy(&output.stderr)));
        }

        let duration_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let duration: f64 = duration_str.parse()?;
        
        Ok(duration as i32)
    }
}
