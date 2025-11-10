use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use uuid::Uuid;

#[async_trait]
pub trait MetricsServiceTrait: Send + Sync {
    async fn create_benchmark_run(&self, source: &str, notes: Option<Value>) -> Result<Uuid>;

    async fn record_video_processing_step(
        &self,
        benchmark_run_id: Option<Uuid>,
        video_id: Option<Uuid>,
        step: &str,
        duration_ms: Option<i64>,
        cpu_avg: Option<f64>,
        mem_peak: Option<i64>,
    ) -> Result<()>;

    async fn record_api_latency_metric(
        &self,
        benchmark_run_id: Option<Uuid>,
        route: &str,
        method: &str,
        status: &str,
        latency_ms: i64,
        concurrent_requests: Option<i32>,
    ) -> Result<()>;

    async fn record_playback_metric(
        &self,
        benchmark_run_id: Option<Uuid>,
        country: Option<&str>,
        isp: Option<&str>,
        device_type: Option<&str>,
        first_frame_ms: Option<i64>,
        total_startup_ms: Option<i64>,
        buffering_events: Option<i32>,
    ) -> Result<()>;

    async fn record_server_startup_metric(
        &self,
        benchmark_run_id: Option<Uuid>,
        service_name: &str,
        revision: Option<&str>,
        cold_start: bool,
        startup_duration_ms: i64,
        context: Option<Value>,
    ) -> Result<()>;

    async fn fetch_insights(&self) -> Result<MetricsInsights>;

    fn base_environment(&self) -> Value;
}

#[derive(Debug, Serialize)]
pub struct MetricsInsights {
    pub video_processing: VideoProcessingInsights,
    pub api_latency: ApiLatencyInsights,
    pub playback: PlaybackInsights,
    pub server_startup: ServerStartupInsights,
}

#[derive(Debug, Serialize, Default)]
pub struct VideoProcessingInsights {
    pub totals: ProcessingAggregate,
    pub step_breakdown: Vec<ProcessingStepStats>,
    pub recent_runs: Vec<ProcessingRunSummary>,
}

#[derive(Debug, Serialize, Default)]
pub struct ProcessingAggregate {
    pub run_count: i64,
    pub avg_total_duration_ms: Option<i64>,
    pub fastest_run_ms: Option<i64>,
    pub slowest_run_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ProcessingStepStats {
    pub step: String,
    pub sample_count: i64,
    pub avg_duration_ms: Option<i64>,
    pub avg_cpu: Option<f64>,
    pub peak_mem_bytes: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ProcessingRunSummary {
    pub id: Uuid,
    pub started_at: DateTime<Utc>,
    pub source: String,
    pub runner_host: String,
    pub cpu_model: Option<String>,
    pub bandwidth_mbps: Option<f64>,
    pub total_duration_ms: i64,
    pub step_count: i64,
    pub avg_cpu: Option<f64>,
    pub peak_mem_bytes: Option<i64>,
}

#[derive(Debug, Serialize, Default)]
pub struct ApiLatencyInsights {
    pub totals: ApiLatencyTotals,
    pub by_route: Vec<ApiRouteLatency>,
}

#[derive(Debug, Serialize, Default)]
pub struct ApiLatencyTotals {
    pub sample_count: i64,
    pub avg_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub p99_latency_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct ApiRouteLatency {
    pub route: String,
    pub method: String,
    pub sample_count: i64,
    pub avg_latency_ms: Option<i64>,
    pub p95_latency_ms: Option<i64>,
    pub p99_latency_ms: Option<i64>,
    pub avg_concurrent: Option<f64>,
    pub top_statuses: Vec<ApiStatusBreakdown>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ApiStatusBreakdown {
    pub status: String,
    pub sample_count: i64,
}

#[derive(Debug, Serialize, Default)]
pub struct PlaybackInsights {
    pub totals: PlaybackTotals,
    pub by_country: Vec<PlaybackGeoSummary>,
    pub by_device: Vec<PlaybackDeviceSummary>,
}

#[derive(Debug, Serialize, Default)]
pub struct PlaybackTotals {
    pub sample_count: i64,
    pub avg_first_frame_ms: Option<i64>,
    pub avg_total_startup_ms: Option<i64>,
    pub avg_buffering_events: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PlaybackGeoSummary {
    pub country: String,
    pub sample_count: i64,
    pub avg_first_frame_ms: Option<i64>,
    pub avg_total_startup_ms: Option<i64>,
    pub avg_buffering_events: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct PlaybackDeviceSummary {
    pub device_type: String,
    pub sample_count: i64,
    pub avg_first_frame_ms: Option<i64>,
    pub avg_total_startup_ms: Option<i64>,
    pub avg_buffering_events: Option<f64>,
}

#[derive(Debug, Serialize, Default)]
pub struct ServerStartupInsights {
    pub totals: ServerStartupTotals,
    pub recent_samples: Vec<ServerStartupSample>,
}

#[derive(Debug, Serialize, Default)]
pub struct ServerStartupTotals {
    pub sample_count: i64,
    pub avg_startup_ms: Option<i64>,
    pub min_startup_ms: Option<i64>,
    pub max_startup_ms: Option<i64>,
    pub cold_start_avg_ms: Option<i64>,
    pub warm_start_avg_ms: Option<i64>,
    pub cold_start_count: i64,
    pub warm_start_count: i64,
}

#[derive(Debug, Serialize)]
pub struct ServerStartupSample {
    pub recorded_at: DateTime<Utc>,
    pub service_name: String,
    pub revision: Option<String>,
    pub cold_start: bool,
    pub startup_duration_ms: i64,
}

#[derive(Clone)]
pub struct MetricsService {
    pool: PgPool,
    hostname: String,
    region: Option<String>,
    service_name: Option<String>,
    cpu_brand: Option<String>,
    total_memory_bytes: Option<u64>,
    boot_time: Option<SystemTime>,
}

impl MetricsService {
    pub fn new(pool: PgPool) -> Arc<dyn MetricsServiceTrait> {
        let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown-host".to_string());
        let region = std::env::var("GOOGLE_CLOUD_REGION")
            .or_else(|_| std::env::var("X_GOOGLE_GCE_REGION"))
            .ok();
        let service_name = std::env::var("K_SERVICE").ok();

        let refresh_kind = RefreshKind::nothing()
            .with_cpu(CpuRefreshKind::everything())
            .with_memory(MemoryRefreshKind::everything());
        let sys = System::new_with_specifics(refresh_kind);

        let cpu_brand = sys
            .cpus()
            .first()
            .map(|cpu| cpu.brand().to_string())
            .filter(|brand| !brand.is_empty());
        let total_memory_bytes = Some(sys.total_memory() * 1024);
        let boot_time =
            Some(SystemTime::UNIX_EPOCH + Duration::from_secs(System::boot_time()));

        Arc::new(Self {
            pool,
            hostname,
            region,
            service_name,
            cpu_brand,
            total_memory_bytes,
            boot_time,
        }) as Arc<dyn MetricsServiceTrait>
    }

    #[allow(unused_mut)]
    fn ensure_context(&self, context: Option<Value>) -> Value {
        let mut base = self.base_environment();
        match context {
            Some(Value::Object(extra)) => {
                if let Some(obj) = base.as_object_mut() {
                    for (key, value) in extra.into_iter() {
                        obj.insert(key, value);
                    }
                }
                base
            }
            Some(other) => {
                if let Some(obj) = base.as_object_mut() {
                    obj.insert("payload".to_string(), other);
                }
                base
            }
            None => base,
        }
    }

    async fn video_processing_insights(&self) -> Result<VideoProcessingInsights> {
        let totals_row = sqlx::query!(
            r#"
                SELECT
                    COALESCE(COUNT(DISTINCT benchmark_run_id), 0)::bigint AS "run_count!",
                    AVG(total_duration_ms)::bigint AS avg_total_duration_ms,
                    MIN(total_duration_ms)::bigint AS fastest_run_ms,
                    MAX(total_duration_ms)::bigint AS slowest_run_ms
                FROM (
                    SELECT benchmark_run_id, SUM(COALESCE(duration_ms, 0)) AS total_duration_ms
                    FROM video_processing_metrics
                    GROUP BY benchmark_run_id
                ) totals
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        let totals = ProcessingAggregate {
            run_count: totals_row.run_count,
            avg_total_duration_ms: totals_row.avg_total_duration_ms,
            fastest_run_ms: totals_row.fastest_run_ms,
            slowest_run_ms: totals_row.slowest_run_ms,
        };

        let step_rows = sqlx::query!(
            r#"
                SELECT
                    step,
                    COUNT(*)::bigint AS "sample_count!",
                    AVG(duration_ms)::bigint AS avg_duration_ms,
                    AVG(cpu_avg)::double precision AS avg_cpu,
                    MAX(mem_peak)::bigint AS peak_mem_bytes
                FROM video_processing_metrics
                GROUP BY step
                ORDER BY avg_duration_ms NULLS LAST, step
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let step_breakdown = step_rows
            .into_iter()
            .map(|row| ProcessingStepStats {
                step: row.step,
                sample_count: row.sample_count,
                avg_duration_ms: row.avg_duration_ms,
                avg_cpu: row.avg_cpu,
                peak_mem_bytes: row.peak_mem_bytes,
            })
            .collect();

        let recent_rows = sqlx::query!(
            r#"
                SELECT
                    br.id,
                    br.started_at,
                    br.source,
                    br.runner_host,
                    br.cpu_model,
                    br.bandwidth_mbps,
                    COALESCE(SUM(vpm.duration_ms), 0)::bigint AS "total_duration_ms!",
                    COALESCE(COUNT(vpm.id), 0)::bigint AS "step_count!",
                    AVG(vpm.cpu_avg)::double precision AS avg_cpu,
                    MAX(vpm.mem_peak)::bigint AS peak_mem_bytes
                FROM benchmark_runs br
                LEFT JOIN video_processing_metrics vpm ON vpm.benchmark_run_id = br.id
                WHERE br.source = 'video_processing'
                GROUP BY br.id
                ORDER BY br.started_at DESC
                LIMIT 10
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let recent_runs = recent_rows
            .into_iter()
            .map(|row| ProcessingRunSummary {
                id: row.id,
                started_at: row.started_at,
                source: row.source,
                runner_host: row.runner_host.unwrap_or_default().to_string(),
                cpu_model: row.cpu_model,
                bandwidth_mbps: row.bandwidth_mbps,
                total_duration_ms: row.total_duration_ms,
                step_count: row.step_count,
                avg_cpu: row.avg_cpu,
                peak_mem_bytes: row.peak_mem_bytes,
            })
            .collect();

        Ok(VideoProcessingInsights {
            totals,
            step_breakdown,
            recent_runs,
        })
    }

    async fn api_latency_insights(&self) -> Result<ApiLatencyInsights> {
        let totals_row = sqlx::query!(
            r#"
                SELECT
                    COUNT(*)::bigint AS "sample_count!",
                    AVG(latency_ms)::bigint AS avg_latency_ms,
                    PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms)::bigint AS p95_latency_ms,
                    PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY latency_ms)::bigint AS p99_latency_ms
                FROM api_latency_metrics
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        let totals = ApiLatencyTotals {
            sample_count: totals_row.sample_count,
            avg_latency_ms: totals_row.avg_latency_ms,
            p95_latency_ms: totals_row.p95_latency_ms,
            p99_latency_ms: totals_row.p99_latency_ms,
        };

        let status_rows = sqlx::query!(
            r#"
                SELECT
                    route,
                    method,
                    status,
                    COUNT(*)::bigint AS "sample_count!"
                FROM api_latency_metrics
                GROUP BY route, method, status
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut status_map: HashMap<(String, String), Vec<ApiStatusBreakdown>> = HashMap::new();
        for row in status_rows {
            let key = (row.route.clone(), row.method.clone());
            status_map
                .entry(key)
                .or_default()
                .push(ApiStatusBreakdown {
                    status: row.status,
                    sample_count: row.sample_count,
                });
        }

        for statuses in status_map.values_mut() {
            statuses.sort_by(|a, b| b.sample_count.cmp(&a.sample_count));
            statuses.truncate(3);
        }

        let route_rows = sqlx::query!(
            r#"
                SELECT
                    route,
                    method,
                    COUNT(*)::bigint AS "sample_count!",
                    AVG(latency_ms)::bigint AS avg_latency_ms,
                    PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms)::bigint AS p95_latency_ms,
                    PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY latency_ms)::bigint AS p99_latency_ms,
                    AVG(concurrent_requests)::double precision AS avg_concurrent
                FROM api_latency_metrics
                GROUP BY route, method
                ORDER BY p95_latency_ms DESC NULLS LAST, "sample_count!" DESC
                LIMIT 15
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let by_route = route_rows
            .into_iter()
            .map(|row| {
                let key = (row.route.clone(), row.method.clone());
                let top_statuses = status_map
                    .get(&key)
                    .cloned()
                    .unwrap_or_default();

                ApiRouteLatency {
                    route: row.route,
                    method: row.method,
                    sample_count: row.sample_count,
                    avg_latency_ms: row.avg_latency_ms,
                    p95_latency_ms: row.p95_latency_ms,
                    p99_latency_ms: row.p99_latency_ms,
                    avg_concurrent: row.avg_concurrent,
                    top_statuses,
                }
            })
            .collect();

        Ok(ApiLatencyInsights { totals, by_route })
    }

    async fn playback_insights(&self) -> Result<PlaybackInsights> {
        let totals_row = sqlx::query!(
            r#"
                SELECT
                    COUNT(*)::bigint AS "sample_count!",
                    AVG(first_frame_ms)::bigint AS avg_first_frame_ms,
                    AVG(total_startup_ms)::bigint AS avg_total_startup_ms,
                    AVG(buffering_events)::double precision AS avg_buffering_events
                FROM playback_metrics
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        let totals = PlaybackTotals {
            sample_count: totals_row.sample_count,
            avg_first_frame_ms: totals_row.avg_first_frame_ms,
            avg_total_startup_ms: totals_row.avg_total_startup_ms,
            avg_buffering_events: totals_row.avg_buffering_events,
        };

        let country_rows = sqlx::query!(
            r#"
                SELECT
                    COALESCE(country, 'Unknown') AS "country!",
                    COUNT(*)::bigint AS "sample_count!",
                    AVG(first_frame_ms)::bigint AS avg_first_frame_ms,
                    AVG(total_startup_ms)::bigint AS avg_total_startup_ms,
                    AVG(buffering_events)::double precision AS avg_buffering_events
                FROM playback_metrics
                GROUP BY COALESCE(country, 'Unknown')
                ORDER BY avg_total_startup_ms NULLS LAST, "sample_count!" DESC
                LIMIT 15
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let by_country = country_rows
            .into_iter()
            .map(|row| PlaybackGeoSummary {
                country: row.country,
                sample_count: row.sample_count,
                avg_first_frame_ms: row.avg_first_frame_ms,
                avg_total_startup_ms: row.avg_total_startup_ms,
                avg_buffering_events: row.avg_buffering_events,
            })
            .collect();

        let device_rows = sqlx::query!(
            r#"
                SELECT
                    COALESCE(device_type, 'Unknown') AS "device_type!",
                    COUNT(*)::bigint AS "sample_count!",
                    AVG(first_frame_ms)::bigint AS avg_first_frame_ms,
                    AVG(total_startup_ms)::bigint AS avg_total_startup_ms,
                    AVG(buffering_events)::double precision AS avg_buffering_events
                FROM playback_metrics
                GROUP BY COALESCE(device_type, 'Unknown')
                ORDER BY avg_total_startup_ms NULLS LAST, "sample_count!" DESC
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let by_device = device_rows
            .into_iter()
            .map(|row| PlaybackDeviceSummary {
                device_type: row.device_type,
                sample_count: row.sample_count,
                avg_first_frame_ms: row.avg_first_frame_ms,
                avg_total_startup_ms: row.avg_total_startup_ms,
                avg_buffering_events: row.avg_buffering_events,
            })
            .collect();

        Ok(PlaybackInsights {
            totals,
            by_country,
            by_device,
        })
    }

    async fn server_startup_insights(&self) -> Result<ServerStartupInsights> {
        let totals_row = sqlx::query!(
            r#"
                SELECT
                    COUNT(*)::bigint AS "sample_count!",
                    AVG(startup_duration_ms)::bigint AS avg_startup_ms,
                    MIN(startup_duration_ms)::bigint AS min_startup_ms,
                    MAX(startup_duration_ms)::bigint AS max_startup_ms,
                    AVG(startup_duration_ms) FILTER (WHERE cold_start)::bigint AS cold_start_avg_ms,
                    AVG(startup_duration_ms) FILTER (WHERE NOT cold_start)::bigint AS warm_start_avg_ms,
                    COALESCE(COUNT(*) FILTER (WHERE cold_start), 0)::bigint AS "cold_start_count!",
                    COALESCE(COUNT(*) FILTER (WHERE NOT cold_start), 0)::bigint AS "warm_start_count!"
                FROM server_startup_metrics
            "#
        )
        .fetch_one(&self.pool)
        .await?;

        let totals = ServerStartupTotals {
            sample_count: totals_row.sample_count,
            avg_startup_ms: totals_row.avg_startup_ms,
            min_startup_ms: totals_row.min_startup_ms,
            max_startup_ms: totals_row.max_startup_ms,
            cold_start_avg_ms: totals_row.cold_start_avg_ms,
            warm_start_avg_ms: totals_row.warm_start_avg_ms,
            cold_start_count: totals_row.cold_start_count,
            warm_start_count: totals_row.warm_start_count,
        };

        let recent_rows = sqlx::query!(
            r#"
                SELECT
                    recorded_at,
                    service_name,
                    revision,
                    cold_start,
                    startup_duration_ms
                FROM server_startup_metrics
                ORDER BY recorded_at DESC
                LIMIT 20
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let recent_samples = recent_rows
            .into_iter()
            .map(|row| ServerStartupSample {
                recorded_at: row.recorded_at,
                service_name: row.service_name,
                revision: row.revision,
                cold_start: row.cold_start.unwrap(),
                startup_duration_ms: row.startup_duration_ms,
            })
            .collect();

        Ok(ServerStartupInsights {
            totals,
            recent_samples,
        })
    }
}

#[async_trait]
impl MetricsServiceTrait for MetricsService {
    #[allow(unused_mut)]
    async fn create_benchmark_run(&self, source: &str, notes: Option<Value>) -> Result<Uuid> {
        let cpu_model_override = notes
            .as_ref()
            .and_then(|value| value.as_object())
            .and_then(|map| map.get("cpu_model"))
            .and_then(|value| value.as_str())
            .map(str::to_string);

        let bandwidth_override = notes
            .as_ref()
            .and_then(|value| value.as_object())
            .and_then(|map| map.get("bandwidth_mbps"))
            .and_then(|value| value.as_f64());

        let cpu_model = cpu_model_override.or_else(|| self.cpu_brand.clone());

        let mut notes_payload = self.base_environment();
        if let Some(extra) = notes {
            match (notes_payload.as_object_mut(), extra) {
                (Some(base), Value::Object(additional)) => {
                    for (key, value) in additional.into_iter() {
                        base.insert(key, value);
                    }
                }
                (Some(base), other) => {
                    base.insert("payload".to_string(), other);
                }
                _ => {}
            }
        }

        let id = sqlx::query_scalar!(
            r#"
                INSERT INTO benchmark_runs (source, runner_host, cpu_model, bandwidth_mbps, notes)
                VALUES ($1, $2, $3, $4, $5)
                RETURNING id
            "#,
            source,
            self.hostname,
            cpu_model,
            bandwidth_override,
            notes_payload
        )
        .fetch_one(&self.pool)
        .await?;

        Ok(id)
    }

    async fn record_video_processing_step(
        &self,
        benchmark_run_id: Option<Uuid>,
        video_id: Option<Uuid>,
        step: &str,
        duration_ms: Option<i64>,
        cpu_avg: Option<f64>,
        mem_peak: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
                INSERT INTO video_processing_metrics
                (benchmark_run_id, video_id, step, duration_ms, cpu_avg, mem_peak)
                VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            benchmark_run_id,
            video_id,
            step,
            duration_ms,
            cpu_avg,
            mem_peak
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn record_api_latency_metric(
        &self,
        benchmark_run_id: Option<Uuid>,
        route: &str,
        method: &str,
        status: &str,
        latency_ms: i64,
        concurrent_requests: Option<i32>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
                INSERT INTO api_latency_metrics
                (benchmark_run_id, route, method, status, latency_ms, concurrent_requests)
                VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            benchmark_run_id,
            route,
            method,
            status,
            latency_ms,
            concurrent_requests
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn record_playback_metric(
        &self,
        benchmark_run_id: Option<Uuid>,
        country: Option<&str>,
        isp: Option<&str>,
        device_type: Option<&str>,
        first_frame_ms: Option<i64>,
        total_startup_ms: Option<i64>,
        buffering_events: Option<i32>,
    ) -> Result<()> {
        sqlx::query!(
            r#"
                INSERT INTO playback_metrics
                (benchmark_run_id, country, isp, device_type, first_frame_ms, total_startup_ms, buffering_events)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            benchmark_run_id,
            country,
            isp,
            device_type,
            first_frame_ms,
            total_startup_ms,
            buffering_events
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn record_server_startup_metric(
        &self,
        benchmark_run_id: Option<Uuid>,
        service_name: &str,
        revision: Option<&str>,
        cold_start: bool,
        startup_duration_ms: i64,
        context: Option<Value>,
    ) -> Result<()> {
        let context_value = self.ensure_context(context);
        sqlx::query!(
            r#"
                INSERT INTO server_startup_metrics
                (benchmark_run_id, service_name, revision, cold_start, startup_duration_ms, context)
                VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            benchmark_run_id,
            service_name,
            revision,
            cold_start,
            startup_duration_ms,
            context_value
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn fetch_insights(&self) -> Result<MetricsInsights> {
        Ok(MetricsInsights {
            video_processing: self.video_processing_insights().await?,
            api_latency: self.api_latency_insights().await?,
            playback: self.playback_insights().await?,
            server_startup: self.server_startup_insights().await?,
        })
    }

    fn base_environment(&self) -> Value {
        json!({
            "hostname": self.hostname,
            "region": self.region,
            "service_name": self.service_name,
            "cpu_brand": self.cpu_brand,
            "total_memory_bytes": self.total_memory_bytes,
            "boot_time": self.boot_time
                .and_then(|bt| bt.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|dur| dur.as_secs()),
            "rust_version": env!("CARGO_PKG_VERSION"),
        })
    }
}
