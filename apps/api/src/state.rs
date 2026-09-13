use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, RwLock},
};

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::models::DatabaseMetricPoint;

const METRIC_RETENTION_SECONDS: i64 = 24 * 60 * 60;
const METRIC_HISTORY_MAX_SAMPLES: usize = 20_000;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub database_pools: Arc<RwLock<HashMap<Uuid, PgPool>>>,
    pub database_metrics: Arc<RwLock<HashMap<Uuid, VecDeque<DatabaseMetricPoint>>>>,
}

impl AppState {
    pub fn new(db: PgPool, config: Arc<Config>) -> Self {
        Self {
            db,
            config,
            database_pools: Arc::new(RwLock::new(HashMap::new())),
            database_metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn cached_database_pool(&self, resource_id: Uuid) -> Option<PgPool> {
        let mut pools = self
            .database_pools
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        match pools.get(&resource_id) {
            Some(pool) if !pool.is_closed() => Some(pool.clone()),
            Some(_) => {
                pools.remove(&resource_id);
                None
            }
            None => None,
        }
    }

    pub fn cache_database_pool(&self, resource_id: Uuid, pool: PgPool) -> PgPool {
        let mut pools = self
            .database_pools
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(existing) = pools.get(&resource_id) {
            if !existing.is_closed() {
                return existing.clone();
            }
        }
        pools.insert(resource_id, pool.clone());
        pool
    }

    pub fn remove_database_pool(&self, resource_id: Uuid) -> Option<PgPool> {
        self.database_pools
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&resource_id)
    }

    pub fn record_database_metric(
        &self,
        resource_id: Uuid,
        sample: DatabaseMetricPoint,
    ) -> Vec<DatabaseMetricPoint> {
        let mut histories = self
            .database_metrics
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let history = histories.entry(resource_id).or_default();

        if history
            .back()
            .is_some_and(|previous| previous.timestamp == sample.timestamp)
        {
            let _ = history.pop_back();
        }
        history.push_back(sample.clone());

        let oldest_timestamp = sample.timestamp.saturating_sub(METRIC_RETENTION_SECONDS);
        while history
            .front()
            .is_some_and(|oldest| oldest.timestamp < oldest_timestamp)
            || history.len() > METRIC_HISTORY_MAX_SAMPLES
        {
            let _ = history.pop_front();
        }

        history.iter().cloned().collect()
    }
}
