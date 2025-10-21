-- Migration to fix video_status enum for SQLx compatibility
-- This migration ensures SQLx can properly recognize the video_status enum type

-- Drop and recreate the enum to ensure proper SQLx recognition
DROP TYPE IF EXISTS video_status CASCADE;

-- Recreate the enum type
CREATE TYPE video_status AS ENUM ('uploading', 'processing', 'ready', 'failed');

-- Recreate the videos table with the enum
DROP TABLE IF EXISTS videos CASCADE;

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
    status video_status DEFAULT 'uploading',
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

-- Add comments for SQLx metadata
COMMENT ON TYPE video_status IS 'Video processing status enum';
COMMENT ON COLUMN videos.status IS 'Current processing status of the video';
COMMENT ON COLUMN videos.duration IS 'Video duration in seconds';
COMMENT ON COLUMN videos.file_size IS 'Original file size in bytes';
COMMENT ON COLUMN videos.hls_playlist_path IS 'Path to the master HLS playlist file';
COMMENT ON COLUMN videos.thumbnail_path IS 'Path to the video thumbnail image';
