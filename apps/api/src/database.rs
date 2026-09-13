use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};
use futures_util::TryStreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sqlparser::{dialect::PostgreSqlDialect, parser::Parser};
use sqlx::{
    Column, Executor, PgPool, Row, TypeInfo,
    postgres::{PgConnectOptions, PgPoolOptions, PgRow},
};
use uuid::Uuid;

use crate::{auth, cluster, error::AppError, projects, security, state::AppState};

const STATUS_READY: &str = "ready";
const PROVIDER_LEGACY_SHARED: &str = "legacy_shared";
const DEFAULT_TABLE_LIMIT: u32 = 50;
const MAX_QUERY_BYTES: usize = 64 * 1024;
const MAX_TABLE_COLUMNS: usize = 50;
const DATABASE_POOL_MAX_CONNECTIONS: u32 = 4;
const DATABASE_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const DATABASE_POOL_MAX_LIFETIME: Duration = Duration::from_secs(1800);

#[derive(Debug, Deserialize, Default)]
pub struct TableListQuery {
    pub search: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TableDataQuery {
    pub table: String,
    pub schema: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateTableRequest {
    pub name: String,
    pub schema: Option<String>,
    pub columns: Vec<CreateColumnRequest>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateColumnRequest {
    pub name: String,
    pub data_type: String,
    pub nullable: Option<bool>,
    pub primary_key: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteQueryRequest {
    pub sql: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseTableResponse {
    pub schema_name: String,
    pub table_name: String,
    pub estimated_rows: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseColumnResponse {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseTableDataResponse {
    pub schema_name: String,
    pub table_name: String,
    pub columns: Vec<DatabaseColumnResponse>,
    pub rows: Vec<Value>,
    pub limit: u32,
    pub offset: u32,
    pub row_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseStatsResponse {
    pub database_name: String,
    pub size_bytes: i64,
    pub connections: i64,
    pub max_connections: i32,
    pub table_count: i64,
    pub estimated_rows: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseConfigResponse {
    pub name: String,
    pub setting: String,
    pub unit: Option<String>,
    pub description: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseQueryResponse {
    pub columns: Vec<String>,
    pub rows: Vec<Value>,
    pub row_count: usize,
    pub affected_rows: u64,
    pub duration_ms: u128,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedTableResponse {
    pub schema_name: String,
    pub table_name: String,
}

#[derive(Debug, sqlx::FromRow)]
struct DatabaseResourceRow {
    database_name: String,
    role_name: String,
    host: String,
    port: i32,
    password_ciphertext: String,
    status: String,
    cluster_provider: String,
    cluster_host: Option<String>,
    cluster_port: Option<i32>,
}

#[derive(Debug, sqlx::FromRow)]
struct TableRow {
    schema_name: String,
    table_name: String,
    estimated_rows: i64,
    size_bytes: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ColumnRow {
    name: String,
    data_type: String,
    nullable: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct StatsRow {
    database_name: String,
    size_bytes: i64,
    connections: i64,
    max_connections: i32,
    table_count: i64,
    estimated_rows: i64,
}

#[derive(Debug, sqlx::FromRow)]
struct ConfigRow {
    name: String,
    setting: String,
    unit: Option<String>,
    description: String,
}

struct TargetDatabase {
    pool: PgPool,
}

pub async fn list_tables(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, resource_id)): Path<(String, String, Uuid)>,
    Query(query): Query<TableListQuery>,
) -> Result<Json<Vec<DatabaseTableResponse>>, AppError> {
    let target = target_database(
        &state,
        &headers,
        &workspace_slug,
        &project_slug,
        resource_id,
    )
    .await?;
    let search = query.search.unwrap_or_default().trim().to_owned();
    if search.chars().count() > 120 {
        return Err(AppError::BadRequest {
            code: "INVALID_TABLE_SEARCH",
            message: "The table search is too long.",
        });
    }

    let rows = sqlx::query_as::<_, TableRow>(
        "SELECT
            n.nspname AS schema_name,
            c.relname AS table_name,
            GREATEST(c.reltuples, 0)::bigint AS estimated_rows,
            pg_total_relation_size(c.oid)::bigint AS size_bytes
         FROM pg_class AS c
         JOIN pg_namespace AS n ON n.oid = c.relnamespace
         WHERE c.relkind IN ('r', 'p')
           AND n.nspname NOT IN ('pg_catalog', 'information_schema')
           AND ($1 = '' OR c.relname ILIKE '%' || $1 || '%')
         ORDER BY n.nspname ASC, c.relname ASC",
    )
    .bind(search)
    .fetch_all(&target.pool)
    .await
    .map_err(|error| database_error("list database tables", error))?;

    Ok(Json(
        rows.into_iter()
            .map(|row| DatabaseTableResponse {
                schema_name: row.schema_name,
                table_name: row.table_name,
                estimated_rows: row.estimated_rows,
                size_bytes: row.size_bytes,
            })
            .collect(),
    ))
}

pub async fn table_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, resource_id)): Path<(String, String, Uuid)>,
    Query(query): Query<TableDataQuery>,
) -> Result<Json<DatabaseTableDataResponse>, AppError> {
    let target = target_database(
        &state,
        &headers,
        &workspace_slug,
        &project_slug,
        resource_id,
    )
    .await?;
    let schema_name = validate_identifier(query.schema.as_deref().unwrap_or("public"), "schema")?;
    let table_name = validate_identifier(&query.table, "table")?;
    ensure_schema_and_table(&target.pool, &schema_name, &table_name).await?;

    let columns = sqlx::query_as::<_, ColumnRow>(
        "SELECT column_name AS name, data_type, (is_nullable = 'YES') AS nullable
         FROM information_schema.columns
         WHERE table_schema = $1 AND table_name = $2
         ORDER BY ordinal_position ASC",
    )
    .bind(&schema_name)
    .bind(&table_name)
    .fetch_all(&target.pool)
    .await
    .map_err(|error| database_error("read database table columns", error))?;

    let limit = query
        .limit
        .unwrap_or(DEFAULT_TABLE_LIMIT)
        .clamp(1, state.config.database_query_max_rows);
    let offset = query.offset.unwrap_or_default();
    let data_sql = format!(
        "SELECT row_to_json(table_row)::text AS row_json
         FROM (SELECT * FROM {}.{} LIMIT $1 OFFSET $2) AS table_row",
        quote_identifier(&schema_name),
        quote_identifier(&table_name),
    );
    let raw_rows = sqlx::query_scalar::<_, String>(&data_sql)
        .bind(i64::from(limit))
        .bind(i64::from(offset))
        .fetch_all(&target.pool)
        .await
        .map_err(|error| database_error("read database table data", error))?;
    let rows = raw_rows
        .into_iter()
        .map(|raw| {
            serde_json::from_str::<Value>(&raw).map_err(|_| {
                AppError::Internal("database returned invalid JSON row data".to_owned())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(DatabaseTableDataResponse {
        schema_name,
        table_name,
        columns: columns
            .into_iter()
            .map(|column| DatabaseColumnResponse {
                name: column.name,
                data_type: column.data_type,
                nullable: column.nullable,
            })
            .collect(),
        row_count: rows.len(),
        rows,
        limit,
        offset,
    }))
}

pub async fn create_table(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, resource_id)): Path<(String, String, Uuid)>,
    Json(input): Json<CreateTableRequest>,
) -> Result<Json<CreatedTableResponse>, AppError> {
    crate::security::require_csrf(&headers, &state.config)?;
    let target = target_database(
        &state,
        &headers,
        &workspace_slug,
        &project_slug,
        resource_id,
    )
    .await?;
    let schema_name = validate_identifier(input.schema.as_deref().unwrap_or("public"), "schema")?;
    let table_name = validate_identifier(&input.name, "table")?;
    reject_system_schema(&schema_name)?;
    if input.columns.is_empty() || input.columns.len() > MAX_TABLE_COLUMNS {
        return Err(AppError::BadRequest {
            code: "INVALID_TABLE_COLUMNS",
            message: "A table needs between one and fifty columns.",
        });
    }
    ensure_schema_exists(&target.pool, &schema_name).await?;

    let mut seen = HashSet::new();
    let mut definitions = Vec::with_capacity(input.columns.len());
    for column in input.columns {
        let name = validate_identifier(&column.name, "column")?;
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(AppError::BadRequest {
                code: "DUPLICATE_COLUMN",
                message: "Column names must be unique.",
            });
        }
        let data_type = allowed_data_type(&column.data_type)?;
        let is_primary_key = column.primary_key.unwrap_or(false);
        let is_nullable = column.nullable.unwrap_or(true);
        let nullability = if is_primary_key {
            " PRIMARY KEY"
        } else if !is_nullable {
            " NOT NULL"
        } else {
            ""
        };
        definitions.push(format!(
            "{} {}{}",
            quote_identifier(&name),
            data_type,
            nullability,
        ));
    }

    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM pg_class AS c
            JOIN pg_namespace AS n ON n.oid = c.relnamespace
            WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')
        )",
    )
    .bind(&schema_name)
    .bind(&table_name)
    .fetch_one(&target.pool)
    .await
    .map_err(|error| database_error("check database table", error))?;
    if exists {
        return Err(AppError::Conflict {
            code: "TABLE_EXISTS",
            message: "A table with this name already exists.",
        });
    }

    let create_sql = format!(
        "CREATE TABLE {}.{} ({})",
        quote_identifier(&schema_name),
        quote_identifier(&table_name),
        definitions.join(", "),
    );
    sqlx::query(&create_sql)
        .execute(&target.pool)
        .await
        .map_err(|error| database_error("create database table", error))?;

    Ok(Json(CreatedTableResponse {
        schema_name,
        table_name,
    }))
}

pub async fn stats(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, resource_id)): Path<(String, String, Uuid)>,
) -> Result<Json<DatabaseStatsResponse>, AppError> {
    let target = target_database(
        &state,
        &headers,
        &workspace_slug,
        &project_slug,
        resource_id,
    )
    .await?;
    let row = sqlx::query_as::<_, StatsRow>(
        "SELECT
            current_database() AS database_name,
            pg_database_size(current_database())::bigint AS size_bytes,
            (SELECT count(*)::bigint FROM pg_stat_activity WHERE datname = current_database()) AS connections,
            current_setting('max_connections')::int AS max_connections,
            (SELECT count(*)::bigint FROM pg_class AS c
                JOIN pg_namespace AS n ON n.oid = c.relnamespace
                WHERE c.relkind IN ('r', 'p')
                  AND n.nspname NOT IN ('pg_catalog', 'information_schema')) AS table_count,
            (SELECT coalesce(sum(greatest(c.reltuples, 0.0)), 0.0)::bigint FROM pg_class AS c
                JOIN pg_namespace AS n ON n.oid = c.relnamespace
                WHERE c.relkind IN ('r', 'p')
                  AND n.nspname NOT IN ('pg_catalog', 'information_schema')) AS estimated_rows",
    )
    .fetch_one(&target.pool)
    .await
    .map_err(|error| database_error("read database stats", error))?;

    Ok(Json(DatabaseStatsResponse {
        database_name: row.database_name,
        size_bytes: row.size_bytes,
        connections: row.connections,
        max_connections: row.max_connections,
        table_count: row.table_count,
        estimated_rows: row.estimated_rows,
    }))
}

pub async fn config(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, resource_id)): Path<(String, String, Uuid)>,
) -> Result<Json<Vec<DatabaseConfigResponse>>, AppError> {
    let target = target_database(
        &state,
        &headers,
        &workspace_slug,
        &project_slug,
        resource_id,
    )
    .await?;
    let rows = sqlx::query_as::<_, ConfigRow>(
        "SELECT name, setting, unit, short_desc AS description
         FROM pg_settings
         WHERE name = ANY($1)
         ORDER BY name ASC",
    )
    .bind(vec![
        "server_version",
        "max_connections",
        "shared_buffers",
        "work_mem",
        "maintenance_work_mem",
        "timezone",
        "statement_timeout",
        "max_wal_size",
    ])
    .fetch_all(&target.pool)
    .await
    .map_err(|error| database_error("read database config", error))?;

    Ok(Json(
        rows.into_iter()
            .map(|row| DatabaseConfigResponse {
                name: row.name,
                setting: row.setting,
                unit: row.unit,
                description: row.description,
            })
            .collect(),
    ))
}

pub async fn execute_query(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, resource_id)): Path<(String, String, Uuid)>,
    Json(input): Json<ExecuteQueryRequest>,
) -> Result<Json<DatabaseQueryResponse>, AppError> {
    crate::security::require_csrf(&headers, &state.config)?;
    let target = target_database(
        &state,
        &headers,
        &workspace_slug,
        &project_slug,
        resource_id,
    )
    .await?;
    let (sql, returns_rows, wraps_rows) = validate_query(&input.sql)?;
    let started = Instant::now();

    if !returns_rows {
        let result = sqlx::query(&sql)
            .execute(&target.pool)
            .await
            .map_err(|error| database_query_error("execute database query", error))?;
        return Ok(Json(DatabaseQueryResponse {
            columns: Vec::new(),
            rows: Vec::new(),
            row_count: 0,
            affected_rows: result.rows_affected(),
            duration_ms: started.elapsed().as_millis(),
            truncated: false,
        }));
    }

    let query_sql = if wraps_rows {
        format!("SELECT row_to_json(query_result)::text AS row_json FROM ({sql}) AS query_result")
    } else {
        sql.clone()
    };
    let mut stream = sqlx::query(&query_sql).fetch(&target.pool);
    let mut rows = Vec::new();
    let mut columns = if wraps_rows {
        target
            .pool
            .describe(&sql)
            .await
            .map_err(|error| database_query_error("describe database query result", error))?
            .columns()
            .iter()
            .map(|column| column.name().to_owned())
            .collect()
    } else {
        Vec::new()
    };
    let max_rows = usize::try_from(state.config.database_query_max_rows)
        .map_err(|_| AppError::internal("invalid database query row limit"))?;
    let mut truncated = false;

    while let Some(row) = stream
        .try_next()
        .await
        .map_err(|error| database_query_error("read database query result", error))?
    {
        if rows.len() >= max_rows {
            truncated = true;
            break;
        }
        if wraps_rows {
            let raw = row
                .try_get::<String, _>("row_json")
                .map_err(|error| database_query_error("decode database query result", error))?;
            let value = serde_json::from_str::<Value>(&raw).map_err(|_| {
                AppError::Internal("database returned invalid JSON query data".to_owned())
            })?;
            if columns.is_empty() {
                columns = value
                    .as_object()
                    .map(|object| object.keys().cloned().collect())
                    .unwrap_or_default();
            }
            rows.push(value);
        } else {
            if columns.is_empty() {
                columns = row
                    .columns()
                    .iter()
                    .map(|column| column.name().to_owned())
                    .collect();
            }
            rows.push(pg_row_to_json(&row));
        }
    }

    Ok(Json(DatabaseQueryResponse {
        row_count: rows.len(),
        columns,
        rows,
        affected_rows: 0,
        duration_ms: started.elapsed().as_millis(),
        truncated,
    }))
}

async fn target_database(
    state: &AppState,
    headers: &HeaderMap,
    workspace_slug: &str,
    project_slug: &str,
    resource_id: Uuid,
) -> Result<TargetDatabase, AppError> {
    let user = auth::authenticate(state, headers).await?;
    let project_id =
        projects::accessible_project_id(state, user.id, workspace_slug, project_slug).await?;
    let resource = sqlx::query_as::<_, DatabaseResourceRow>(
        "SELECT database_name, role_name, host, port, password_ciphertext,
                status, cluster_provider, cluster_host, cluster_port
         FROM project_postgres_databases
         WHERE id = $1 AND project_id = $2",
    )
    .bind(resource_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "DATABASE_NOT_FOUND",
        message: "The requested database was not found.",
    })?;

    if resource.cluster_provider == PROVIDER_LEGACY_SHARED {
        return Err(AppError::Conflict {
            code: "LEGACY_SHARED_CLUSTER",
            message: "This resource was created on a shared cluster and must be migrated before it can be used with dedicated clusters.",
        });
    }
    if resource.status != STATUS_READY {
        return Err(AppError::ServiceUnavailable {
            code: "DATABASE_NOT_READY",
            message: "The database is not ready yet.",
        });
    }

    let host = cluster::connection_host(
        &resource.cluster_provider,
        resource.cluster_host.as_deref().unwrap_or(&resource.host),
    );
    let port_value = resource.cluster_port.unwrap_or(resource.port);
    let port = u16::try_from(port_value)
        .map_err(|_| AppError::internal("invalid database cluster port"))?;

    if let Some(pool) = state.cached_database_pool(resource_id) {
        return Ok(TargetDatabase { pool });
    }

    let password = security::decrypt_secret(
        &resource.password_ciphertext,
        &state.config.database_credentials_encryption_key,
    )?;
    let options = PgConnectOptions::new()
        .host(&host)
        .port(port)
        .database(&resource.database_name)
        .username(&resource.role_name)
        .password(&password);
    let timeout = format!("{}ms", state.config.database_query_timeout_ms);
    let pool = PgPoolOptions::new()
        .max_connections(DATABASE_POOL_MAX_CONNECTIONS)
        .min_connections(0)
        .idle_timeout(DATABASE_POOL_IDLE_TIMEOUT)
        .max_lifetime(DATABASE_POOL_MAX_LIFETIME)
        .acquire_timeout(Duration::from_secs(5))
        .after_connect(move |connection, _metadata| {
            let timeout = timeout.clone();
            Box::pin(async move {
                sqlx::query("SELECT set_config('statement_timeout', $1, false)")
                    .bind(timeout)
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await
        .map_err(|error| {
            tracing::warn!(resource_id = %resource_id, error = %error, "could not connect to project database");
            AppError::ServiceUnavailable {
                code: "DATABASE_UNAVAILABLE",
                message: "The project database is temporarily unavailable.",
            }
        })?;

    Ok(TargetDatabase {
        pool: state.cache_database_pool(resource_id, pool),
    })
}

async fn ensure_schema_and_table(
    pool: &PgPool,
    schema_name: &str,
    table_name: &str,
) -> Result<(), AppError> {
    reject_system_schema(schema_name)?;
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
            SELECT 1 FROM pg_class AS c
            JOIN pg_namespace AS n ON n.oid = c.relnamespace
            WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')
        )",
    )
    .bind(schema_name)
    .bind(table_name)
    .fetch_one(pool)
    .await
    .map_err(|error| database_error("check database table", error))?;
    if !exists {
        return Err(AppError::NotFound {
            code: "TABLE_NOT_FOUND",
            message: "The requested table was not found.",
        });
    }
    Ok(())
}

async fn ensure_schema_exists(pool: &PgPool, schema_name: &str) -> Result<(), AppError> {
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_namespace WHERE nspname = $1)",
    )
    .bind(schema_name)
    .fetch_one(pool)
    .await
    .map_err(|error| database_error("check database schema", error))?;
    if !exists {
        return Err(AppError::NotFound {
            code: "SCHEMA_NOT_FOUND",
            message: "The requested schema was not found.",
        });
    }
    Ok(())
}

fn validate_identifier(value: &str, field: &'static str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 63 || value.chars().any(char::is_control) {
        return Err(AppError::BadRequest {
            code: "INVALID_DATABASE_IDENTIFIER",
            message: match field {
                "schema" => "Schema names must be between one and sixty-three characters.",
                "table" => "Table names must be between one and sixty-three characters.",
                _ => "Column names must be between one and sixty-three characters.",
            },
        });
    }
    Ok(value.to_owned())
}

fn reject_system_schema(schema_name: &str) -> Result<(), AppError> {
    if schema_name == "pg_catalog"
        || schema_name == "information_schema"
        || schema_name.starts_with("pg_toast")
    {
        return Err(AppError::Forbidden {
            code: "SYSTEM_SCHEMA_FORBIDDEN",
            message: "System schemas cannot be changed through this API.",
        });
    }
    Ok(())
}

fn quote_identifier(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('\"', "\"\""))
}

fn allowed_data_type(data_type: &str) -> Result<&'static str, AppError> {
    let normalized = data_type
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    let canonical = match normalized.as_str() {
        "boolean" | "bool" => "boolean",
        "smallint" | "int2" => "smallint",
        "integer" | "int" | "int4" => "integer",
        "bigint" | "int8" => "bigint",
        "numeric" | "decimal" => "numeric",
        "real" | "float4" => "real",
        "double precision" | "float8" => "double precision",
        "text" => "text",
        "varchar" | "character varying" => "varchar",
        "date" => "date",
        "timestamp" | "timestamp without time zone" => "timestamp",
        "timestamptz" | "timestamp with time zone" => "timestamptz",
        "uuid" => "uuid",
        "json" => "json",
        "jsonb" => "jsonb",
        "bytea" => "bytea",
        _ => {
            return Err(AppError::BadRequest {
                code: "UNSUPPORTED_COLUMN_TYPE",
                message: "This column type is not supported by the table builder.",
            });
        }
    };
    Ok(canonical)
}

fn validate_query(input: &str) -> Result<(String, bool, bool), AppError> {
    let trimmed = input.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_QUERY_BYTES {
        return Err(AppError::BadRequest {
            code: "INVALID_QUERY",
            message: "Enter one SQL statement up to 64 KiB.",
        });
    }
    let statements =
        Parser::parse_sql(&PostgreSqlDialect {}, trimmed).map_err(|_| AppError::BadRequest {
            code: "INVALID_QUERY",
            message: "The SQL statement could not be parsed.",
        })?;
    if statements.len() != 1 {
        return Err(AppError::BadRequest {
            code: "MULTI_STATEMENT_QUERY",
            message: "Run one SQL statement at a time.",
        });
    }
    let statement = &statements[0];
    if restricted_statement(statement) {
        return Err(AppError::Forbidden {
            code: "RESTRICTED_QUERY",
            message: "This SQL operation is not available through the database console.",
        });
    }
    let sql = trimmed.trim_end_matches(';').trim().to_owned();
    let keyword = sql
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .trim_start_matches('(')
        .to_ascii_lowercase();
    let wraps_rows = matches!(keyword.as_str(), "select" | "with" | "values" | "table");
    let returns_rows = wraps_rows
        || matches!(
            keyword.as_str(),
            "show" | "explain" | "fetch" | "describe" | "declare"
        );
    Ok((sql, returns_rows, wraps_rows))
}

fn restricted_statement(statement: &sqlparser::ast::Statement) -> bool {
    use sqlparser::ast::{ObjectType, Statement};
    match statement {
        Statement::Set(_)
        | Statement::Reset(_)
        | Statement::Copy { .. }
        | Statement::CreateRole(_)
        | Statement::CreateDatabase { .. }
        | Statement::CreateServer(_)
        | Statement::CreateFunction(_)
        | Statement::CreateProcedure { .. }
        | Statement::AlterRole { .. }
        | Statement::Grant(_)
        | Statement::Revoke(_)
        | Statement::Execute { .. }
        | Statement::Call(_)
        | Statement::Load { .. }
        | Statement::DropFunction(_)
        | Statement::DropProcedure { .. } => true,
        Statement::Drop { object_type, .. } => matches!(
            object_type,
            ObjectType::Database | ObjectType::Role | ObjectType::User
        ),
        _ => false,
    }
}

fn pg_row_to_json(row: &PgRow) -> Value {
    let mut object = Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        object.insert(
            column.name().to_owned(),
            pg_value_to_json(row, index, column.type_info().name()),
        );
    }
    Value::Object(object)
}

fn pg_value_to_json(row: &PgRow, index: usize, type_name: &str) -> Value {
    match type_name {
        "BOOL" => row
            .try_get::<Option<bool>, _>(index)
            .ok()
            .flatten()
            .map_or(Value::Null, Value::Bool),
        "INT2" => row
            .try_get::<Option<i16>, _>(index)
            .ok()
            .flatten()
            .map_or(Value::Null, |value| Value::from(i64::from(value))),
        "INT4" => row
            .try_get::<Option<i32>, _>(index)
            .ok()
            .flatten()
            .map_or(Value::Null, |value| Value::from(i64::from(value))),
        "INT8" => row
            .try_get::<Option<i64>, _>(index)
            .ok()
            .flatten()
            .map_or(Value::Null, Value::from),
        "FLOAT4" => row
            .try_get::<Option<f32>, _>(index)
            .ok()
            .flatten()
            .and_then(|value| serde_json::Number::from_f64(f64::from(value)))
            .map_or(Value::Null, Value::Number),
        "FLOAT8" => row
            .try_get::<Option<f64>, _>(index)
            .ok()
            .flatten()
            .and_then(serde_json::Number::from_f64)
            .map_or(Value::Null, Value::Number),
        "JSON" | "JSONB" => row
            .try_get::<Option<Value>, _>(index)
            .ok()
            .flatten()
            .unwrap_or(Value::Null),
        "BYTEA" => row
            .try_get::<Option<Vec<u8>>, _>(index)
            .ok()
            .flatten()
            .map(|value| {
                Value::String(format!(
                    "\\x{}",
                    value
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<String>()
                ))
            })
            .unwrap_or(Value::Null),
        _ => row
            .try_get::<Option<String>, _>(index)
            .ok()
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::Null),
    }
}

fn database_error(operation: &'static str, error: sqlx::Error) -> AppError {
    tracing::warn!(operation, error = %error, "project database operation failed");
    AppError::ServiceUnavailable {
        code: "DATABASE_OPERATION_FAILED",
        message: "The project database rejected the operation or is temporarily unavailable.",
    }
}

fn database_query_error(operation: &'static str, error: sqlx::Error) -> AppError {
    let code = error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .map(|code| code.into_owned());
    match code.as_deref() {
        Some("42P01") | Some("3F000") => {
            tracing::info!(operation, error = %error, "database query referenced a missing relation");
            AppError::BadRequest {
                code: "QUERY_RELATION_NOT_FOUND",
                message: "The query references a table or schema that does not exist.",
            }
        }
        Some("42703") => {
            tracing::info!(operation, error = %error, "database query referenced a missing column");
            AppError::BadRequest {
                code: "QUERY_COLUMN_NOT_FOUND",
                message: "The query references a column that does not exist.",
            }
        }
        Some("42601") => {
            tracing::info!(operation, error = %error, "database query has invalid syntax");
            AppError::BadRequest {
                code: "QUERY_SYNTAX_ERROR",
                message: "The database could not parse this SQL statement.",
            }
        }
        _ => database_error(operation, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers_without_allowing_sql_injection() {
        assert_eq!(quote_identifier("orders"), "\"orders\"");
        assert_eq!(quote_identifier("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn accepts_safe_builder_types_only() {
        assert_eq!(
            allowed_data_type("double   precision").unwrap(),
            "double precision"
        );
        assert!(allowed_data_type("text; DROP TABLE users").is_err());
    }

    #[test]
    fn validates_one_statement_and_restricts_cluster_operations() {
        let (_, returns_rows, wraps_rows) = validate_query("SELECT 1;").unwrap();
        assert!(returns_rows);
        assert!(wraps_rows);
        assert!(validate_query("SELECT 1; SELECT 2;").is_err());
        assert!(validate_query("CREATE ROLE another_user").is_err());
        assert!(validate_query("SET search_path TO public").is_err());
    }

    #[test]
    fn validates_database_identifiers() {
        assert_eq!(validate_identifier(" public ", "schema").unwrap(), "public");
        assert!(validate_identifier("", "table").is_err());
        assert!(validate_identifier(&"x".repeat(64), "column").is_err());
    }
}
