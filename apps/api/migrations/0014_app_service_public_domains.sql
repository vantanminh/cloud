ALTER TABLE project_app_services
    ADD COLUMN public_subdomain TEXT;

UPDATE project_app_services
SET public_subdomain = 'app-' || left(replace(id::text, '-', ''), 16)
WHERE public_subdomain IS NULL;

ALTER TABLE project_app_services
    ALTER COLUMN public_subdomain SET NOT NULL;

ALTER TABLE project_app_services
    ADD CONSTRAINT project_app_services_public_subdomain_key UNIQUE (public_subdomain),
    ADD CONSTRAINT project_app_services_public_subdomain_format_check
        CHECK (public_subdomain ~ '^[a-z0-9][a-z0-9-]{0,62}$');
