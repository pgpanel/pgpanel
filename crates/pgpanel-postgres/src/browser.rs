use serde_json::Value;
use sqlx::{Column, Row, TypeInfo};

use pgpanel_core::error::{Error, Result};
use pgpanel_core::models::{
    ColumnInfo, ConstraintInfo, ForeignKeyInfo, IndexInfo, SchemaInfo, TableDetails, TableInfo,
    TableRowsQuery, TableRowsResponse,
};
use pgpanel_core::validation::{quote_ident, validate_safe_name};

use crate::client::PgClient;

const MAX_PAGE_SIZE: u32 = 500;
const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_TEXT_LEN: usize = 512;

pub struct BrowserService<'a> {
    client: &'a PgClient,
}

impl<'a> BrowserService<'a> {
    pub fn new(client: &'a PgClient) -> Self {
        Self { client }
    }

    pub async fn list_schemas(&self) -> Result<Vec<SchemaInfo>> {
        let rows = sqlx::query(
            r#"
            SELECT schema_name AS name
            FROM information_schema.schemata
            WHERE schema_name NOT IN ('pg_catalog', 'information_schema', 'pg_toast')
              AND schema_name NOT LIKE 'pg_toast%'
              AND schema_name NOT LIKE 'pg_temp%'
            ORDER BY schema_name
            "#,
        )
        .fetch_all(self.client.pool())
        .await
        .map_err(|e| Error::Postgres(format!("list schemas: {e}")))?;

        Ok(rows
            .into_iter()
            .filter_map(|r| {
                r.try_get::<String, _>("name")
                    .ok()
                    .map(|name| SchemaInfo { name })
            })
            .collect())
    }

    pub async fn list_tables(&self, schema: Option<&str>) -> Result<Vec<TableInfo>> {
        let rows = if let Some(schema) = schema {
            validate_safe_name(schema, "schema").ok();
            sqlx::query(
                r#"
                SELECT n.nspname AS schema, c.relname AS name,
                       CASE c.relkind WHEN 'r' THEN 'table' WHEN 'v' THEN 'view'
                            WHEN 'm' THEN 'materialized_view' WHEN 'f' THEN 'foreign'
                            WHEN 'p' THEN 'partitioned' ELSE c.relkind::text END AS table_type,
                       GREATEST(c.reltuples::bigint, 0) AS row_estimate
                FROM pg_class c
                JOIN pg_namespace n ON n.oid = c.relnamespace
                WHERE c.relkind IN ('r','v','m','f','p')
                  AND n.nspname = $1
                ORDER BY c.relname
                "#,
            )
            .bind(schema)
            .fetch_all(self.client.pool())
            .await
        } else {
            sqlx::query(
                r#"
                SELECT n.nspname AS schema, c.relname AS name,
                       CASE c.relkind WHEN 'r' THEN 'table' WHEN 'v' THEN 'view'
                            WHEN 'm' THEN 'materialized_view' WHEN 'f' THEN 'foreign'
                            WHEN 'p' THEN 'partitioned' ELSE c.relkind::text END AS table_type,
                       GREATEST(c.reltuples::bigint, 0) AS row_estimate
                FROM pg_class c
                JOIN pg_namespace n ON n.oid = c.relnamespace
                WHERE c.relkind IN ('r','v','m','f','p')
                  AND n.nspname NOT IN ('pg_catalog', 'information_schema')
                  AND n.nspname NOT LIKE 'pg_toast%'
                ORDER BY n.nspname, c.relname
                "#,
            )
            .fetch_all(self.client.pool())
            .await
        }
        .map_err(|e| Error::Postgres(format!("list tables: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| TableInfo {
                schema: r.try_get("schema").unwrap_or_default(),
                name: r.try_get("name").unwrap_or_default(),
                table_type: r.try_get("table_type").unwrap_or_default(),
                row_estimate: r.try_get("row_estimate").ok(),
            })
            .collect())
    }

    pub async fn table_details(&self, schema: &str, table: &str) -> Result<TableDetails> {
        validate_safe_name(schema, "schema")?;
        validate_safe_name(table, "table")?;

        let columns = self.columns(schema, table).await?;
        let foreign_keys = self.foreign_keys(schema, table).await?;
        let indexes = self.indexes(schema, table).await?;
        let constraints = self.constraints(schema, table).await?;

        Ok(TableDetails {
            schema: schema.to_string(),
            name: table.to_string(),
            columns,
            foreign_keys,
            indexes,
            constraints,
        })
    }

    async fn columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> {
        let rows = sqlx::query(
            r#"
            SELECT
                a.attname AS name,
                pg_catalog.format_type(a.atttypid, a.atttypmod) AS data_type,
                NOT a.attnotnull AS is_nullable,
                pg_get_expr(ad.adbin, ad.adrelid) AS column_default,
                EXISTS (
                    SELECT 1 FROM pg_index i
                    WHERE i.indrelid = a.attrelid AND a.attnum = ANY(i.indkey) AND i.indisprimary
                ) AS is_primary_key
            FROM pg_attribute a
            JOIN pg_class c ON c.oid = a.attrelid
            JOIN pg_namespace n ON n.oid = c.relnamespace
            LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
            WHERE n.nspname = $1 AND c.relname = $2
              AND a.attnum > 0 AND NOT a.attisdropped
            ORDER BY a.attnum
            "#,
        )
        .bind(schema)
        .bind(table)
        .fetch_all(self.client.pool())
        .await
        .map_err(|e| Error::Postgres(format!("columns: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| ColumnInfo {
                name: r.try_get("name").unwrap_or_default(),
                data_type: r.try_get("data_type").unwrap_or_default(),
                is_nullable: r.try_get("is_nullable").unwrap_or(true),
                column_default: r.try_get("column_default").ok(),
                is_primary_key: r.try_get("is_primary_key").unwrap_or(false),
            })
            .collect())
    }

    async fn foreign_keys(&self, schema: &str, table: &str) -> Result<Vec<ForeignKeyInfo>> {
        let rows = sqlx::query(
            r#"
            SELECT
                tc.constraint_name AS name,
                kcu.column_name AS column,
                ccu.table_schema AS foreign_table_schema,
                ccu.table_name AS foreign_table,
                ccu.column_name AS foreign_column
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
              ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
            JOIN information_schema.constraint_column_usage ccu
              ON ccu.constraint_name = tc.constraint_name AND ccu.table_schema = tc.table_schema
            WHERE tc.constraint_type = 'FOREIGN KEY'
              AND tc.table_schema = $1 AND tc.table_name = $2
            "#,
        )
        .bind(schema)
        .bind(table)
        .fetch_all(self.client.pool())
        .await
        .map_err(|e| Error::Postgres(format!("foreign keys: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| ForeignKeyInfo {
                name: r.try_get("name").unwrap_or_default(),
                column: r.try_get("column").unwrap_or_default(),
                foreign_table_schema: r.try_get("foreign_table_schema").unwrap_or_default(),
                foreign_table: r.try_get("foreign_table").unwrap_or_default(),
                foreign_column: r.try_get("foreign_column").unwrap_or_default(),
            })
            .collect())
    }

    async fn indexes(&self, schema: &str, table: &str) -> Result<Vec<IndexInfo>> {
        let rows = sqlx::query(
            r#"
            SELECT
                i.relname AS name,
                pg_get_indexdef(i.oid) AS definition,
                ix.indisunique AS is_unique,
                ix.indisprimary AS is_primary
            FROM pg_index ix
            JOIN pg_class t ON t.oid = ix.indrelid
            JOIN pg_class i ON i.oid = ix.indexrelid
            JOIN pg_namespace n ON n.oid = t.relnamespace
            WHERE n.nspname = $1 AND t.relname = $2
            ORDER BY i.relname
            "#,
        )
        .bind(schema)
        .bind(table)
        .fetch_all(self.client.pool())
        .await
        .map_err(|e| Error::Postgres(format!("indexes: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| IndexInfo {
                name: r.try_get("name").unwrap_or_default(),
                definition: r.try_get("definition").unwrap_or_default(),
                is_unique: r.try_get("is_unique").unwrap_or(false),
                is_primary: r.try_get("is_primary").unwrap_or(false),
            })
            .collect())
    }

    async fn constraints(&self, schema: &str, table: &str) -> Result<Vec<ConstraintInfo>> {
        let rows = sqlx::query(
            r#"
            SELECT
                con.conname AS name,
                CASE con.contype
                    WHEN 'p' THEN 'PRIMARY KEY'
                    WHEN 'u' THEN 'UNIQUE'
                    WHEN 'f' THEN 'FOREIGN KEY'
                    WHEN 'c' THEN 'CHECK'
                    WHEN 'x' THEN 'EXCLUDE'
                    ELSE con.contype::text
                END AS constraint_type,
                pg_get_constraintdef(con.oid) AS definition
            FROM pg_constraint con
            JOIN pg_class rel ON rel.oid = con.conrelid
            JOIN pg_namespace nsp ON nsp.oid = rel.relnamespace
            WHERE nsp.nspname = $1 AND rel.relname = $2
            ORDER BY con.conname
            "#,
        )
        .bind(schema)
        .bind(table)
        .fetch_all(self.client.pool())
        .await
        .map_err(|e| Error::Postgres(format!("constraints: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|r| ConstraintInfo {
                name: r.try_get("name").unwrap_or_default(),
                constraint_type: r.try_get("constraint_type").unwrap_or_default(),
                definition: r.try_get("definition").unwrap_or_default(),
            })
            .collect())
    }

    pub async fn table_rows(
        &self,
        schema: &str,
        table: &str,
        query: &TableRowsQuery,
    ) -> Result<TableRowsResponse> {
        validate_safe_name(schema, "schema")?;
        validate_safe_name(table, "table")?;

        let page_size = query.page_size.clamp(1, MAX_PAGE_SIZE).min(MAX_PAGE_SIZE);
        let page_size = if page_size == 0 {
            DEFAULT_PAGE_SIZE
        } else {
            page_size
        };
        let page = query.page.max(1);
        let offset = (page - 1) as u64 * page_size as u64;

        let schema_q = quote_ident(schema)?;
        let table_q = quote_ident(table)?;

        let mut order = String::new();
        if let Some(col) = &query.sort_column {
            validate_safe_name(col, "sort_column")?;
            let col_q = quote_ident(col)?;
            let dir = match query.sort_dir.as_deref() {
                Some("desc") | Some("DESC") => "DESC",
                _ => "ASC",
            };
            order = format!(" ORDER BY {col_q} {dir}");
        }

        let mut filter = String::new();
        let mut bind_filter: Option<String> = None;
        if let (Some(col), Some(val)) = (&query.filter_column, &query.filter_value) {
            validate_safe_name(col, "filter_column")?;
            let col_q = quote_ident(col)?;
            filter = format!(" WHERE {col_q}::text = $1");
            bind_filter = Some(val.clone());
        }

        let sql = format!(
            "SELECT * FROM {schema_q}.{table_q}{filter}{order} LIMIT {page_size} OFFSET {offset}"
        );

        // statement timeout for browser queries
        let mut tx = self
            .client
            .pool()
            .begin()
            .await
            .map_err(|e| Error::Postgres(format!("begin: {e}")))?;
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Postgres(e.to_string()))?;
        sqlx::query("SET LOCAL statement_timeout = '15s'")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Postgres(e.to_string()))?;
        sqlx::query("SET LOCAL lock_timeout = '3s'")
            .execute(&mut *tx)
            .await
            .map_err(|e| Error::Postgres(e.to_string()))?;

        let rows = if let Some(val) = bind_filter {
            sqlx::query(&sql).bind(val).fetch_all(&mut *tx).await
        } else {
            sqlx::query(&sql).fetch_all(&mut *tx).await
        }
        .map_err(|e| Error::Postgres(format!("table rows: {e}")))?;

        tx.rollback()
            .await
            .map_err(|e| Error::Postgres(e.to_string()))?;

        let columns: Vec<String> = if let Some(first) = rows.first() {
            first
                .columns()
                .iter()
                .map(|c| c.name().to_string())
                .collect()
        } else {
            Vec::new()
        };

        let mut truncated_fields = false;
        let mut out_rows = Vec::new();
        for row in &rows {
            let mut vals = Vec::new();
            for (i, col) in row.columns().iter().enumerate() {
                let (v, trunc) = cell_to_json(row, i, col.type_info().name());
                if trunc {
                    truncated_fields = true;
                }
                vals.push(v);
            }
            out_rows.push(vals);
        }

        Ok(TableRowsResponse {
            columns,
            rows: out_rows,
            page,
            page_size,
            total_estimate: None,
            truncated_fields,
        })
    }
}

fn cell_to_json(row: &sqlx::postgres::PgRow, idx: usize, type_name: &str) -> (Value, bool) {
    if let Ok(v) = row.try_get::<Option<bool>, _>(idx) {
        return (
            match v {
                Some(b) => Value::Bool(b),
                None => Value::Null,
            },
            false,
        );
    }
    if let Ok(v) = row.try_get::<Option<i64>, _>(idx) {
        return (
            match v {
                Some(n) => Value::Number(n.into()),
                None => Value::Null,
            },
            false,
        );
    }
    if let Ok(v) = row.try_get::<Option<i32>, _>(idx) {
        return (
            match v {
                Some(n) => Value::Number(n.into()),
                None => Value::Null,
            },
            false,
        );
    }
    if let Ok(v) = row.try_get::<Option<String>, _>(idx) {
        return match v {
            Some(s) if s.len() > MAX_TEXT_LEN => (
                Value::String(format!(
                    "{}…[truncated {} bytes]",
                    &s[..MAX_TEXT_LEN],
                    s.len()
                )),
                true,
            ),
            Some(s) => (Value::String(s), false),
            None => (Value::Null, false),
        };
    }
    if let Ok(v) = row.try_get::<Option<Vec<u8>>, _>(idx) {
        return match v {
            Some(b) => (
                Value::String(format!("\\x[binary {} bytes]", b.len())),
                true,
            ),
            None => (Value::Null, false),
        };
    }
    let _ = type_name;
    (Value::String(format!("[{type_name}]")), false)
}
