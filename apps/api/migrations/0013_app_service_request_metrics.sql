ALTER TABLE app_service_metric_samples
    ADD COLUMN public_network_receive_bytes BIGINT,
    ADD COLUMN public_network_transmit_bytes BIGINT,
    ADD COLUMN requests BIGINT,
    ADD COLUMN response_time_ms DOUBLE PRECISION,
    ADD COLUMN request_error_rate DOUBLE PRECISION;
