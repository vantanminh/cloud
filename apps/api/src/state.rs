use std::{
    collections::HashMap,
    sync::{Arc, Mutex, RwLock},
};

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;

#[derive(Debug, Clone, Copy, Default)]
pub struct AppServiceTrafficSnapshot {
    pub public_network_receive_bytes: u64,
    pub public_network_transmit_bytes: u64,
    pub requests: u64,
    pub response_time_ms_total: f64,
    pub request_errors: u64,
}

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub database_pools: Arc<RwLock<HashMap<Uuid, PgPool>>>,
    pub github_registry_lock: Arc<tokio::sync::Mutex<()>>,
    pub public_app_service_traffic: Arc<Mutex<HashMap<Uuid, AppServiceTrafficSnapshot>>>,
    pub public_proxy_client: reqwest::Client,
}

impl AppState {
    pub fn new(db: PgPool, config: Arc<Config>) -> Self {
        Self {
            db,
            config,
            database_pools: Arc::new(RwLock::new(HashMap::new())),
            github_registry_lock: Arc::new(tokio::sync::Mutex::new(())),
            public_app_service_traffic: Arc::new(Mutex::new(HashMap::new())),
            public_proxy_client: reqwest::Client::new(),
        }
    }

    pub fn record_public_app_service_request(
        &self,
        service_id: Uuid,
        request_bytes: usize,
        response_bytes: usize,
        response_time_ms: f64,
        is_error: bool,
    ) {
        let mut traffic = self
            .public_app_service_traffic
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let snapshot = traffic.entry(service_id).or_default();
        snapshot.public_network_receive_bytes = snapshot
            .public_network_receive_bytes
            .saturating_add(request_bytes as u64);
        snapshot.public_network_transmit_bytes = snapshot
            .public_network_transmit_bytes
            .saturating_add(response_bytes as u64);
        snapshot.requests = snapshot.requests.saturating_add(1);
        snapshot.response_time_ms_total += response_time_ms.max(0.0);
        if is_error {
            snapshot.request_errors = snapshot.request_errors.saturating_add(1);
        }
    }

    pub fn public_app_service_traffic(&self, service_id: Uuid) -> AppServiceTrafficSnapshot {
        self.public_app_service_traffic
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&service_id)
            .copied()
            .unwrap_or_default()
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
}
