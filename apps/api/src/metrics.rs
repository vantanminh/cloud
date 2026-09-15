use std::time::{SystemTime, UNIX_EPOCH};

use crate::{error::AppError, models::ResourceMetricPoint};

pub const MAX_METRIC_RESPONSE_POINTS: usize = 300;
pub const METRIC_SAMPLE_INTERVAL_SECONDS: u64 = 5;
pub const METRIC_RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const METRIC_SAMPLE_MAX_CONCURRENCY: usize = 4;

pub fn metric_sample_concurrency(database_max_connections: u32) -> usize {
    usize::try_from(database_max_connections)
        .unwrap_or(1)
        .saturating_sub(2)
        .clamp(1, METRIC_SAMPLE_MAX_CONCURRENCY)
}

#[derive(Debug, Clone, Copy)]
pub struct MetricRange {
    pub key: &'static str,
    pub seconds: i64,
}

impl MetricRange {
    pub fn bucket_seconds(self) -> i64 {
        (self.seconds + MAX_METRIC_RESPONSE_POINTS as i64 - 1)
            .div_euclid(MAX_METRIC_RESPONSE_POINTS as i64)
            .max(METRIC_SAMPLE_INTERVAL_SECONDS as i64)
    }
}

pub fn parse_metric_range(value: Option<&str>) -> Result<MetricRange, AppError> {
    match value.unwrap_or("24h").trim() {
        "1h" => Ok(MetricRange {
            key: "1h",
            seconds: 60 * 60,
        }),
        "6h" => Ok(MetricRange {
            key: "6h",
            seconds: 6 * 60 * 60,
        }),
        "24h" | "1d" => Ok(MetricRange {
            key: "24h",
            seconds: 24 * 60 * 60,
        }),
        "7d" | "1w" => Ok(MetricRange {
            key: "7d",
            seconds: 7 * 24 * 60 * 60,
        }),
        "30d" | "1m" => Ok(MetricRange {
            key: "30d",
            seconds: METRIC_RETENTION_SECONDS,
        }),
        _ => Err(AppError::BadRequest {
            code: "INVALID_METRIC_RANGE",
            message: "Metric range must be one of 1h, 6h, 24h, 7d, or 30d.",
        }),
    }
}

pub fn unix_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or_default()
}

pub fn has_system_metrics(sample: &ResourceMetricPoint) -> bool {
    sample.cpu_percent.is_some()
        || sample.memory_used_bytes.is_some()
        || sample.network_receive_bytes.is_some()
        || sample.network_transmit_bytes.is_some()
        || sample.disk_read_bytes.is_some()
        || sample.disk_write_bytes.is_some()
}

pub fn downsample_metric_points(
    points: Vec<ResourceMetricPoint>,
    max_points: usize,
) -> Vec<ResourceMetricPoint> {
    if max_points < 2 || points.len() <= max_points {
        return points;
    }

    let last_index = points.len() - 1;
    (0..max_points)
        .map(|index| {
            let source_index = index * last_index / (max_points - 1);
            points[source_index].clone()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_bounded_metric_ranges_and_computes_chart_resolution() {
        assert_eq!(parse_metric_range(None).unwrap().key, "24h");
        assert_eq!(parse_metric_range(Some("1h")).unwrap().seconds, 3_600);
        assert_eq!(parse_metric_range(Some("1w")).unwrap().key, "7d");
        assert_eq!(parse_metric_range(Some("1m")).unwrap().key, "30d");
        assert_eq!(
            parse_metric_range(Some("30d")).unwrap().bucket_seconds(),
            8_640
        );
        assert!(parse_metric_range(Some("90d")).is_err());
    }

    #[test]
    fn reserves_control_pool_connections_for_foreground_work() {
        assert_eq!(metric_sample_concurrency(1), 1);
        assert_eq!(metric_sample_concurrency(4), 2);
        assert_eq!(metric_sample_concurrency(10), 4);
        assert_eq!(metric_sample_concurrency(100), 4);
    }
}
