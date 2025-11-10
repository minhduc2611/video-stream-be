-- Add sample_count columns to metrics tables
ALTER TABLE playback_metrics
    ADD COLUMN IF NOT EXISTS sample_count BIGINT NOT NULL DEFAULT 0;

ALTER TABLE api_latency_metrics
    ADD COLUMN IF NOT EXISTS sample_count BIGINT NOT NULL DEFAULT 0;

ALTER TABLE video_processing_metrics
    ADD COLUMN IF NOT EXISTS sample_count BIGINT NOT NULL DEFAULT 0;

ALTER TABLE server_startup_metrics
    ADD COLUMN IF NOT EXISTS sample_count BIGINT NOT NULL DEFAULT 0;

