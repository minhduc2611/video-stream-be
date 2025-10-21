-- Migration to replace video_status enum with VARCHAR for SQLx compatibility
-- This migration changes the status column to use VARCHAR instead of enum

-- Drop the videos table and recreate with VARCHAR status
DROP TABLE IF EXISTS videos CASCADE;

-- Drop the enum type
DROP TYPE IF EXISTS video_status CASCADE;

-- Recreate the videos table with VARCHAR status
CREATE TABLE videos (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title VARCHAR(200) NOT NULL,
    description TEXT,
    filename VARCHAR(255) NOT NULL,
    original_filename VARCHAR(255) NOT NULL,
    file_size BIGINT NOT NULL,
    duration INTEGER, -- Duration in seconds
    thumbnail_path VARCHAR(500),
    hls_playlist_path VARCHAR(500),
    status VARCHAR(20) DEFAULT 'uploading' CHECK (status IN ('uploading', 'processing', 'ready', 'failed')),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- Recreate indexes for better performance
CREATE INDEX idx_videos_user_id ON videos(user_id);
CREATE INDEX idx_videos_status ON videos(status);
CREATE INDEX idx_videos_created_at ON videos(created_at DESC);

-- Recreate triggers to automatically update updated_at
CREATE TRIGGER update_videos_updated_at BEFORE UPDATE ON videos
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- Add comments for documentation
COMMENT ON COLUMN videos.status IS 'Video processing status: uploading, processing, ready, failed';
COMMENT ON COLUMN videos.duration IS 'Video duration in seconds';
COMMENT ON COLUMN videos.file_size IS 'Original file size in bytes';
COMMENT ON COLUMN videos.hls_playlist_path IS 'Path to the master HLS playlist file';
COMMENT ON COLUMN videos.thumbnail_path IS 'Path to the video thumbnail image';
