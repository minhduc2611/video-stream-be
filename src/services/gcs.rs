use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use google_cloud_storage::client::Storage;
use std::env;
use std::path::Path;
use tokio::fs;
use uuid::Uuid;

#[async_trait]
pub trait CloudStorageService: Send + Sync {
    async fn upload_file_data(&self, file_data: Vec<u8>, remote_path: &str) -> Result<String>;
    async fn download_file(&self, remote_path: &str, local_path: &str) -> Result<()>;
    async fn delete_file(&self, remote_path: &str) -> Result<()>;
    fn get_public_url(&self, remote_path: &str) -> String;
    fn get_signed_url(&self, remote_path: &str, expiration_hours: u32) -> Result<String>;
    fn get_video_path(&self, video_id: &Uuid, filename: &str) -> String;
    fn get_thumbnail_path(&self, video_id: &Uuid) -> String;
    fn get_hls_path(&self, video_id: &Uuid) -> String;
}

#[derive(Clone)]
pub struct GcsService {
    client: Storage,
    bucket_name: String,
    project_id: String,
}

impl GcsService {
    pub async fn new() -> Result<Self> {
        log::info!("📁Initializing GCS service");
        let bucket_name = env::var("GOOGLE_CLOUD_STORAGE_BUCKET")
            .map_err(|_| anyhow::anyhow!("GOOGLE_CLOUD_STORAGE_BUCKET not set"))?;

        log::info!("GCS bucket name: {}", bucket_name);

        let project_id = env::var("GOOGLE_CLOUD_PROJECT_ID")
            .map_err(|_| anyhow::anyhow!("GOOGLE_CLOUD_PROJECT_ID not set"))?;
        log::info!("GCS project id: {}", project_id);

        // Initialize GCS client
        let client = Storage::builder()
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize GCS client: {}", e))?;
        log::info!("📦GCS client initialized");
        Ok(Self {
            client,
            bucket_name,
            project_id,
        })
    }
}

#[async_trait]
impl CloudStorageService for GcsService {
    async fn upload_file_data(&self, file_data: Vec<u8>, remote_path: &str) -> Result<String> {
        log::info!(
            "Uploading file data ({} bytes) to gs://{}/{}",
            file_data.len(),
            self.bucket_name,
            remote_path
        );

        // Upload file data directly to GCS using Bytes
        let bucket_path = format!("projects/_/buckets/{}", self.bucket_name);
        self.client
            .write_object(&bucket_path, remote_path, Bytes::from(file_data))
            .send_unbuffered()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to upload file data to GCS: {}", e))?;

        log::info!("Successfully uploaded {} to GCS", remote_path);
        Ok(format!("gs://{}/{}", self.bucket_name, remote_path))
    }

    async fn download_file(&self, remote_path: &str, local_path: &str) -> Result<()> {
        log::info!(
            "Downloading file from gs://{}/{} to {}",
            self.bucket_name,
            remote_path,
            local_path
        );

        // Download file from GCS using the builder pattern
        let bucket_path = format!("projects/_/buckets/{}", self.bucket_name);
        let mut reader = self
            .client
            .read_object(&bucket_path, remote_path)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to download file from GCS: {}", e))?;

        // Collect all chunks into a vector
        let mut contents = Vec::new();
        while let Some(chunk) = reader
            .next()
            .await
            .transpose()
            .map_err(|e| anyhow::anyhow!("Failed to read chunk from stream: {}", e))?
        {
            contents.extend_from_slice(&chunk);
        }

        // Write file content to local path
        fs::write(local_path, contents)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write file to {}: {}", local_path, e))?;

        log::info!("Successfully downloaded {} from GCS", remote_path);
        Ok(())
    }

    async fn delete_file(&self, remote_path: &str) -> Result<()> {
        log::info!("Deleting file gs://{}/{}", self.bucket_name, remote_path);

        // TODO: Implement actual GCS delete when the API becomes available
        // The current version of google-cloud-storage crate doesn't have delete functionality
        // For now, we'll just log the operation
        log::warn!("Delete operation not implemented in current GCS client version");

        log::info!("Delete operation logged for {} from GCS", remote_path);
        Ok(())
    }

    fn get_public_url(&self, remote_path: &str) -> String {
        format!(
            "https://storage.googleapis.com/{}/{}",
            self.bucket_name, remote_path
        )
    }

    fn get_signed_url(&self, remote_path: &str, expiration_hours: u32) -> Result<String> {
        // TODO: Implement signed URL generation
        // This would require proper GCS client setup with service account credentials
        Ok(self.get_public_url(remote_path))
    }

    fn get_video_path(&self, video_id: &Uuid, filename: &str) -> String {
        format!("{}/videos/{}", video_id, filename)
    }

    fn get_thumbnail_path(&self, video_id: &Uuid) -> String {
        format!("{}/thumbnails/thumbnail.jpg", video_id)
    }

    fn get_hls_path(&self, video_id: &Uuid) -> String {
        format!("{}/hls/", video_id)
    }
}

impl GcsService {
    /// Get content type based on file extension
    fn get_content_type(&self, file_path: &str) -> String {
        let extension = Path::new(file_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "mp4" => "video/mp4".to_string(),
            "mov" => "video/quicktime".to_string(),
            "avi" => "video/x-msvideo".to_string(),
            "mkv" => "video/x-matroska".to_string(),
            "webm" => "video/webm".to_string(),
            "flv" => "video/x-flv".to_string(),
            "wmv" => "video/x-ms-wmv".to_string(),
            "m4v" => "video/x-m4v".to_string(),
            "jpg" | "jpeg" => "image/jpeg".to_string(),
            "png" => "image/png".to_string(),
            "gif" => "image/gif".to_string(),
            "m3u8" => "application/vnd.apple.mpegurl".to_string(),
            "ts" => "video/mp2t".to_string(),
            _ => "application/octet-stream".to_string(),
        }
    }
}
