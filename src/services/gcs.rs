use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use google_cloud_storage::client::{Storage, StorageControl};

use std::env;
use std::path::Path;
use tokio::fs;
use uuid::Uuid;

#[async_trait]
pub trait CloudStorageService: Send + Sync {
    async fn upload_file_data(&self, file_data: Vec<u8>, remote_path: &str) -> Result<String>;
    async fn download_file(&self, remote_path: &str, local_path: &str) -> Result<()>;
    async fn delete_folder(&self, folder_prefix: &str) -> Result<()>;
    fn get_public_url(&self, remote_path: &str) -> String;
    #[allow(dead_code)]
    fn get_signed_url(&self, remote_path: &str, expiration_hours: u32) -> Result<String>;
    fn get_video_path(&self, video_id: &Uuid, filename: &str) -> String;
    fn get_thumbnail_path(&self, video_id: &Uuid) -> String;
    fn get_hls_path(&self, video_id: &Uuid) -> String;
}

#[derive(Clone)]
pub struct GcsService {
    storage_client: Storage,
    control_client: StorageControl,
    bucket_name: String,
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
        let control_client = StorageControl::builder()
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to initialize GCS control client: {}", e))?;
        log::info!("🗂️ GCS control client initialized");
        Ok(Self {
            storage_client: client,
            control_client,
            bucket_name,
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
        let cache_control = Self::determine_cache_control(remote_path);
        let content_type = Self::get_content_type(remote_path);

        self.storage_client
            .write_object(&bucket_path, remote_path, Bytes::from(file_data))
            .set_cache_control(cache_control)
            .set_content_type(content_type)
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
            .storage_client
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

    /// Delete every object whose name starts with the provided `folder_prefix`.
    ///
    /// We treat the prefix like a virtual directory: first try to remove an object
    /// that exactly matches the prefix (some uploads store the original asset there),
    /// then list all objects whose name begins with `prefix/` and delete them one by one.
    /// This works on standard flat GCS buckets without requiring hierarchical namespaces.
    async fn delete_folder(&self, folder_prefix: &str) -> Result<()> {
        let normalized = folder_prefix.trim_matches('/');

        if normalized.is_empty() {
            log::warn!("delete_folder called with empty prefix; skipping");
            return Ok(());
        }

        let bucket_resource = format!("projects/_/buckets/{}", self.bucket_name);
        let root_object_name = normalized.to_string();
        let prefix = format!("{}/", normalized);

        log::info!(
            "Deleting all objects with prefix gs://{}/{}",
            self.bucket_name,
            prefix
        );

        // Attempt to delete an object that exactly matches the prefix (without trailing slash)
        if let Err(e) = self
            .control_client
            .delete_object()
            .set_bucket(bucket_resource.clone())
            .set_object(root_object_name.clone())
            .send()
            .await
        {
            log::debug!(
                "No direct object named {} to delete (or failed): {}",
                root_object_name,
                e
            );
        } else {
            log::info!("Deleted object {} during prefix cleanup", root_object_name);
        }

        let mut page_token = String::new();

        loop {
            let mut request = self
                .control_client
                .list_objects()
                .set_parent(bucket_resource.clone())
                .set_prefix(prefix.clone());

            if !page_token.is_empty() {
                request = request.set_page_token(page_token.clone());
            }

            let response = request.send().await.map_err(|e| {
                anyhow::anyhow!(
                    "Failed to list objects for prefix {} in bucket {}: {}",
                    prefix,
                    self.bucket_name,
                    e
                )
            })?;

            for object in response.objects {
                let object_name = object.name;

                self.control_client
                    .delete_object()
                    .set_bucket(bucket_resource.clone())
                    .set_object(object_name.clone())
                    .send()
                    .await
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to delete object {} during prefix cleanup: {}",
                            object_name,
                            e
                        )
                    })?;

                log::info!("Deleted object {} during prefix cleanup", object_name);
            }

            if response.next_page_token.is_empty() {
                break;
            }

            page_token = response.next_page_token;
        }

        log::info!(
            "Completed prefix cleanup for gs://{}/{}",
            self.bucket_name,
            prefix
        );

        Ok(())
    }

    fn get_public_url(&self, remote_path: &str) -> String {
        format!(
            "https://storage.googleapis.com/{}/{}",
            self.bucket_name, remote_path
        )
    }

    #[allow(dead_code)]
    fn get_signed_url(&self, remote_path: &str, _expiration_hours: u32) -> Result<String> {
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
    #[allow(dead_code)]
    fn get_content_type(file_path: &str) -> String {
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

    fn determine_cache_control(file_path: &str) -> String {
        let extension = Path::new(file_path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_lowercase();

        match extension.as_str() {
            "m3u8" => "public, max-age=1, no-transform".to_string(),
            "ts" | "mp4" => "public, max-age=86400".to_string(),
            _ => "no-cache".to_string(),
        }
    }
}
