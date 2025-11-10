-- Update benchmark_runs to new schema
ALTER TABLE benchmark_runs
    DROP COLUMN IF EXISTS runner_region,
    DROP COLUMN IF EXISTS environment,
    DROP COLUMN IF EXISTS metadata;

ALTER TABLE benchmark_runs
    ADD COLUMN IF NOT EXISTS cpu_model TEXT,
    ADD COLUMN IF NOT EXISTS bandwidth_mbps DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS notes JSONB DEFAULT '{}'::jsonb;

-- Align video_processing_metrics with new metrics fields
ALTER TABLE video_processing_metrics
    DROP COLUMN IF EXISTS cpu_usage_percent,
    DROP COLUMN IF EXISTS memory_bytes;

ALTER TABLE video_processing_metrics
    ADD COLUMN IF NOT EXISTS cpu_avg DOUBLE PRECISION,
    ADD COLUMN IF NOT EXISTS mem_peak BIGINT;

-- Update api_latency_metrics columns
ALTER TABLE api_latency_metrics
    DROP COLUMN IF EXISTS context;

ALTER TABLE api_latency_metrics
    RENAME COLUMN status_code TO status;

ALTER TABLE api_latency_metrics
    ALTER COLUMN status TYPE TEXT USING status::text;

-- Update playback_metrics columns
ALTER TABLE playback_metrics
    DROP COLUMN IF EXISTS video_id,
    DROP COLUMN IF EXISTS connection_type,
    DROP COLUMN IF EXISTS delivery_source,
    DROP COLUMN IF EXISTS bandwidth_mbps,
    DROP COLUMN IF EXISTS startup_ms,
    DROP COLUMN IF EXISTS context;

ALTER TABLE playback_metrics
    ADD COLUMN IF NOT EXISTS total_startup_ms BIGINT;

