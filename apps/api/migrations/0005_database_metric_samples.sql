CREATE TABLE database_metric_samples (
    resource_id UUID NOT NULL REFERENCES project_postgres_databases(id) ON DELETE CASCADE,
    sampled_at TIMESTAMPTZ NOT NULL,
    cpu_percent DOUBLE PRECISION,
    memory_used_bytes BIGINT,
    memory_limit_bytes BIGINT,
    volume_used_bytes BIGINT,
    volume_capacity_bytes BIGINT,
    network_receive_bytes BIGINT,
    network_transmit_bytes BIGINT,
    disk_read_bytes BIGINT,
    disk_write_bytes BIGINT,
    PRIMARY KEY (resource_id, sampled_at)
);

CREATE INDEX database_metric_samples_resource_sampled_at_idx
    ON database_metric_samples (resource_id, sampled_at DESC);
