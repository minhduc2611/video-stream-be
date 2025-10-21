use anyhow::Result;
use std::path::Path;
use uuid::Uuid;

#[derive(Clone)]
pub struct StorageService {
    upload_dir: String,
}

impl StorageService {
    pub fn new(upload_dir: String) -> Self {
        Self { upload_dir }
    }

    pub fn get_video_path(&self, video_id: &Uuid, filename: &str) -> String {
        format!("{}/videos/{}/{}", self.upload_dir, video_id, filename)
    }

    pub fn get_thumbnail_path(&self, video_id: &Uuid) -> String {
        format!("{}/thumbnails/{}.jpg", self.upload_dir, video_id)
    }

    pub fn get_hls_path(&self, video_id: &Uuid) -> String {
        format!("{}/hls/{}/", self.upload_dir, video_id)
    }

    pub fn get_hls_file_path(&self, video_id: &Uuid, filename: &str) -> String {
        format!("{}/hls/{}/{}", self.upload_dir, video_id, filename)
    }

    pub async fn create_video_directory(&self, video_id: &Uuid) -> Result<()> {
        let video_dir = format!("{}/videos/{}", self.upload_dir, video_id);
        let hls_dir = format!("{}/hls/{}", self.upload_dir, video_id);
        let thumbnail_dir = format!("{}/thumbnails", self.upload_dir);

        tokio::fs::create_dir_all(&video_dir).await?;
        tokio::fs::create_dir_all(&hls_dir).await?;
        tokio::fs::create_dir_all(&thumbnail_dir).await?;

        Ok(())
    }

    pub async fn save_uploaded_file(&self, video_id: &Uuid, filename: &str, data: &[u8]) -> Result<String> {
        let file_path = self.get_video_path(video_id, filename);
        let parent_dir = Path::new(&file_path).parent().unwrap();
        
        tokio::fs::create_dir_all(parent_dir).await?;
        tokio::fs::write(&file_path, data).await?;

        log::info!("Saved file to: {}", file_path);
        Ok(file_path)
    }

    pub async fn save_video_file(&self, video_id: &Uuid, filename: &str, data: &[u8]) -> Result<String> {
        self.save_uploaded_file(video_id, filename, data).await
    }

    pub async fn file_exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

    pub async fn delete_file(&self, path: &str) -> Result<()> {
        if self.file_exists(path).await {
            tokio::fs::remove_file(path).await?;
        }
        Ok(())
    }

    pub async fn delete_directory(&self, path: &str) -> Result<()> {
        if Path::new(path).exists() {
            tokio::fs::remove_dir_all(path).await?;
        }
        Ok(())
    }
}
