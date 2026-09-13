use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;

#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub database_pools: Arc<RwLock<HashMap<Uuid, PgPool>>>,
}

impl AppState {
    pub fn new(db: PgPool, config: Arc<Config>) -> Self {
        Self {
            db,
            config,
            database_pools: Arc::new(RwLock::new(HashMap::new())),
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
}
