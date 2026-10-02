CREATE TABLE image_stores (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    compression_mode TEXT NOT NULL CHECK (compression_mode IN ('none', 'fixed', 'per_url')),
    max_width INTEGER CHECK (max_width IS NULL OR max_width BETWEEN 1 AND 8192),
    max_height INTEGER CHECK (max_height IS NULL OR max_height BETWEEN 1 AND 8192),
    quality INTEGER CHECK (quality IS NULL OR quality BETWEEN 1 AND 100),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX image_stores_project_id_idx ON image_stores (project_id);

CREATE TABLE image_api_keys (
    id UUID PRIMARY KEY,
    store_id UUID NOT NULL REFERENCES image_stores(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    client_id TEXT NOT NULL UNIQUE,
    secret_hash BYTEA NOT NULL,
    access TEXT NOT NULL CHECK (access IN ('full', 'browser')),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'revoked')),
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX image_api_keys_store_id_idx ON image_api_keys (store_id);

CREATE TABLE image_objects (
    id UUID PRIMARY KEY,
    store_id UUID NOT NULL REFERENCES image_stores(id) ON DELETE CASCADE,
    folder TEXT NOT NULL DEFAULT '',
    file_name TEXT NOT NULL CHECK (char_length(file_name) BETWEEN 1 AND 180),
    content_type TEXT NOT NULL,
    byte_size BIGINT NOT NULL CHECK (byte_size > 0),
    width INTEGER NOT NULL CHECK (width > 0),
    height INTEGER NOT NULL CHECK (height > 0),
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (store_id, folder, file_name)
);

CREATE INDEX image_objects_store_folder_idx ON image_objects (store_id, folder);
