CREATE TABLE app_service_metric_samples (
    app_service_id UUID NOT NULL REFERENCES project_app_services(id) ON DELETE CASCADE,
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
    PRIMARY KEY (app_service_id, sampled_at)
);

CREATE INDEX app_service_metric_samples_service_sampled_at_idx
    ON app_service_metric_samples (app_service_id, sampled_at DESC);
