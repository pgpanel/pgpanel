//! Audit action constants for dangerous operations.

pub const CLUSTER_CREATE: &str = "cluster.create";
pub const CLUSTER_DELETE: &str = "cluster.delete";
pub const CLUSTER_START: &str = "cluster.start";
pub const CLUSTER_STOP: &str = "cluster.stop";
pub const CLUSTER_RESTART: &str = "cluster.restart";
pub const VOLUME_DELETE: &str = "volume.delete";
pub const DATABASE_CREATE: &str = "database.create";
pub const DATABASE_DELETE: &str = "database.delete";
pub const ROLE_CREATE: &str = "role.create";
pub const ROLE_DELETE: &str = "role.delete";
pub const ROLE_PASSWORD_CHANGE: &str = "role.password_change";
pub const PORT_PUBLISH: &str = "cluster.port_publish";
pub const BACKUP_CONFIG: &str = "backup.config_change";
pub const BACKUP_TRIGGER: &str = "backup.trigger";
pub const AUTH_LOGIN: &str = "auth.login";
pub const AUTH_LOGOUT: &str = "auth.logout";
pub const AUTH_BOOTSTRAP: &str = "auth.bootstrap";
pub const SQL_QUERY: &str = "sql.query";
