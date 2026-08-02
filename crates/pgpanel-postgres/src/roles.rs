use secrecy::{ExposeSecret, SecretString};
use sqlx::Row;
use tracing::info;

use pgpanel_core::error::{Error, Result};
use pgpanel_core::validation::{quote_ident, validate_safe_name};

use crate::client::PgClient;

pub struct RoleService<'a> {
    client: &'a PgClient,
}

impl<'a> RoleService<'a> {
    pub fn new(client: &'a PgClient) -> Self {
        Self { client }
    }

    /// Create a non-privileged LOGIN role.
    pub async fn create_role(
        &self,
        role_name: &str,
        password: &SecretString,
        connection_limit: Option<i32>,
    ) -> Result<()> {
        validate_safe_name(role_name, "role_name")?;
        let ident = quote_ident(role_name)?;
        let limit_clause = match connection_limit {
            Some(n) if n > 0 => format!(" CONNECTION LIMIT {n}"),
            _ => String::new(),
        };

        // Password is interpolated carefully: Escape single quotes in password.
        // We never log the password.
        let pw_escaped = password.expose_secret().replace('\'', "''");
        let sql = format!(
            "CREATE ROLE {ident} WITH LOGIN PASSWORD '{pw_escaped}' NOSUPERUSER NOCREATEDB NOCREATEROLE NOREPLICATION NOBYPASSRLS{limit_clause}"
        );

        info!(%role_name, "creating PostgreSQL role");
        sqlx::query(&sql)
            .execute(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("create role: {e}")))?;
        Ok(())
    }

    pub async fn role_exists(&self, role_name: &str) -> Result<bool> {
        validate_safe_name(role_name, "role_name")?;
        let row = sqlx::query("SELECT 1 AS ok FROM pg_roles WHERE rolname = $1")
            .bind(role_name)
            .fetch_optional(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("role exists: {e}")))?;
        Ok(row.is_some())
    }

    pub async fn change_password(&self, role_name: &str, password: &SecretString) -> Result<()> {
        validate_safe_name(role_name, "role_name")?;
        let ident = quote_ident(role_name)?;
        let pw_escaped = password.expose_secret().replace('\'', "''");
        let sql = format!("ALTER ROLE {ident} WITH PASSWORD '{pw_escaped}'");
        info!(%role_name, "changing role password");
        sqlx::query(&sql)
            .execute(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("alter password: {e}")))?;
        Ok(())
    }

    pub async fn drop_role(&self, role_name: &str) -> Result<()> {
        validate_safe_name(role_name, "role_name")?;
        if role_name == "postgres" {
            return Err(Error::Forbidden("cannot drop postgres superuser".into()));
        }
        let ident = quote_ident(role_name)?;
        // REASSIGN / DROP OWNED first would be safer for owned objects.
        let sql = format!("DROP ROLE IF EXISTS {ident}");
        info!(%role_name, "dropping role");
        sqlx::query(&sql)
            .execute(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("drop role: {e}")))?;
        Ok(())
    }

    /// Create database owned by role. CREATE DATABASE cannot run in a transaction.
    pub async fn create_database(
        &self,
        database_name: &str,
        owner_role: &str,
        connection_limit: Option<i32>,
    ) -> Result<()> {
        validate_safe_name(database_name, "database_name")?;
        validate_safe_name(owner_role, "owner_role")?;
        let db_ident = quote_ident(database_name)?;
        let owner_ident = quote_ident(owner_role)?;
        let limit_clause = match connection_limit {
            Some(n) if n > 0 => format!(" CONNECTION LIMIT {n}"),
            _ => String::new(),
        };

        let sql = format!("CREATE DATABASE {db_ident} OWNER {owner_ident}{limit_clause}");
        info!(%database_name, %owner_role, "creating database");
        sqlx::query(&sql)
            .execute(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("create database: {e}")))?;

        // Revoke PUBLIC connect / create on the new DB (best practice).
        // Connect as admin to the new DB for privilege cleanup.
        self.revoke_public_privileges(database_name, owner_role)
            .await?;

        Ok(())
    }

    async fn revoke_public_privileges(&self, database_name: &str, owner_role: &str) -> Result<()> {
        let db_ident = quote_ident(database_name)?;
        let owner_ident = quote_ident(owner_role)?;

        // These run on the admin connection against postgres DB.
        let stmts = [
            format!("REVOKE ALL ON DATABASE {db_ident} FROM PUBLIC"),
            format!("GRANT CONNECT, TEMPORARY ON DATABASE {db_ident} TO {owner_ident}"),
        ];
        for sql in stmts {
            sqlx::query(&sql)
                .execute(self.client.pool())
                .await
                .map_err(|e| Error::Postgres(format!("revoke public: {e}")))?;
        }
        Ok(())
    }

    pub async fn database_exists(&self, database_name: &str) -> Result<bool> {
        validate_safe_name(database_name, "database_name")?;
        let row = sqlx::query("SELECT 1 AS ok FROM pg_database WHERE datname = $1")
            .bind(database_name)
            .fetch_optional(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("database exists: {e}")))?;
        Ok(row.is_some())
    }

    pub async fn drop_database(&self, database_name: &str) -> Result<()> {
        validate_safe_name(database_name, "database_name")?;
        if database_name == "postgres"
            || database_name == "template0"
            || database_name == "template1"
        {
            return Err(Error::Forbidden("cannot drop system database".into()));
        }
        let ident = quote_ident(database_name)?;
        // Terminate backends first
        let term = "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE datname = $1 AND pid <> pg_backend_pid()";
        let _ = sqlx::query(term)
            .bind(database_name)
            .execute(self.client.pool())
            .await;

        let sql = format!("DROP DATABASE IF EXISTS {ident}");
        info!(%database_name, "dropping database");
        sqlx::query(&sql)
            .execute(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("drop database: {e}")))?;
        Ok(())
    }

    /// Create a backup role with limited privileges for Databasus.
    pub async fn create_backup_role(&self, password: &SecretString) -> Result<()> {
        let role = "pgpanel_backup";
        if self.role_exists(role).await? {
            self.change_password(role, password).await?;
            return Ok(());
        }
        let pw_escaped = password.expose_secret().replace('\'', "''");
        // REPLICATION may be needed for some backup tools; keep minimal for MVP.
        let sql = format!(
            "CREATE ROLE \"{role}\" WITH LOGIN REPLICATION PASSWORD '{pw_escaped}' NOSUPERUSER NOCREATEDB NOCREATEROLE"
        );
        sqlx::query(&sql)
            .execute(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("create backup role: {e}")))?;

        // Grant connect on all DBs
        let dbs = sqlx::query("SELECT datname FROM pg_database WHERE datistemplate = false")
            .fetch_all(self.client.pool())
            .await
            .map_err(|e| Error::Postgres(format!("list dbs for backup grant: {e}")))?;
        for row in dbs {
            let name: String = row.try_get("datname").unwrap_or_default();
            if let Ok(ident) = quote_ident(&name) {
                let g = format!("GRANT CONNECT ON DATABASE {ident} TO \"{role}\"");
                let _ = sqlx::query(&g).execute(self.client.pool()).await;
            }
        }
        Ok(())
    }

    pub async fn list_roles(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"
            SELECT rolname FROM pg_roles
            WHERE rolname NOT LIKE 'pg_%'
            ORDER BY rolname
            "#,
        )
        .fetch_all(self.client.pool())
        .await
        .map_err(|e| Error::Postgres(format!("list roles: {e}")))?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.try_get::<String, _>("rolname").ok())
            .collect())
    }

    pub async fn list_databases(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"
            SELECT datname FROM pg_database
            WHERE datistemplate = false
            ORDER BY datname
            "#,
        )
        .fetch_all(self.client.pool())
        .await
        .map_err(|e| Error::Postgres(format!("list databases: {e}")))?;
        Ok(rows
            .into_iter()
            .filter_map(|r| r.try_get::<String, _>("datname").ok())
            .collect())
    }
}
