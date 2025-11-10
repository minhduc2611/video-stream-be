CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- Benchmark run sessions
CREATE TABLE IF NOT EXISTS benchmark_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    started_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    source TEXT NOT NULL,
    runner_host TEXT,
    runner_region TEXT,
    environment JSONB DEFAULT '{}'::jsonb,
    metadata JSONB DEFAULT '{}'::jsonb
);

-- Video processing step metrics
CREATE TABLE IF NOT EXISTS video_processing_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    benchmark_run_id UUID REFERENCES benchmark_runs(id) ON DELETE SET NULL,
    video_id UUID,
    step TEXT NOT NULL,
    duration_ms BIGINT,
    cpu_usage_percent NUMERIC,
    memory_bytes BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    context JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_video_processing_metrics_video_id
    ON video_processing_metrics(video_id);

-- API latency metrics
CREATE TABLE IF NOT EXISTS api_latency_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    benchmark_run_id UUID REFERENCES benchmark_runs(id) ON DELETE SET NULL,
    route TEXT NOT NULL,
    method TEXT NOT NULL,
    status_code INT NOT NULL,
    latency_ms BIGINT NOT NULL,
    concurrent_requests INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    context JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_api_latency_metrics_route
    ON api_latency_metrics(route);

-- Playback telemetry
CREATE TABLE IF NOT EXISTS playback_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    benchmark_run_id UUID REFERENCES benchmark_runs(id) ON DELETE SET NULL,
    video_id UUID,
    country TEXT,
    isp TEXT,
    device_type TEXT,
    connection_type TEXT,
    delivery_source TEXT,
    bandwidth_mbps NUMERIC,
    first_frame_ms BIGINT,
    startup_ms BIGINT,
    buffering_events INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    context JSONB DEFAULT '{}'::jsonb
);

CREATE INDEX IF NOT EXISTS idx_playback_metrics_video_id
    ON playback_metrics(video_id);

-- Server startup metrics
CREATE TABLE IF NOT EXISTS server_startup_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    benchmark_run_id UUID REFERENCES benchmark_runs(id) ON DELETE SET NULL,
    service_name TEXT NOT NULL,
    revision TEXT,
    cold_start BOOLEAN DEFAULT FALSE,
    startup_duration_ms BIGINT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    context JSONB DEFAULT '{}'::jsonb
);

