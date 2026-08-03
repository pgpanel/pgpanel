//! Role and permission definitions.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Panel roles ordered by privilege.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Full control including user management.
    Owner,
    /// Administrative access without ownership transfer.
    Administrator,
    /// Day-to-day cluster and database operations.
    Operator,
    /// Read-only access.
    Viewer,
}

impl Role {
    /// Parse from database string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "owner" => Some(Self::Owner),
            "administrator" => Some(Self::Administrator),
            "operator" => Some(Self::Operator),
            "viewer" => Some(Self::Viewer),
            _ => None,
        }
    }

    /// Database string.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Owner => "owner",
            Self::Administrator => "administrator",
            Self::Operator => "operator",
            Self::Viewer => "viewer",
        }
    }
}

/// Granular permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// List/inspect clusters.
    ClustersRead,
    /// Create clusters.
    ClustersCreate,
    /// Start/stop/restart/reload.
    ClustersControl,
    /// Edit allowlisted config.
    ClustersConfigure,
    /// Delete clusters.
    ClustersDelete,
    /// List databases.
    DatabasesRead,
    /// Create/rename databases.
    DatabasesCreate,
    /// Drop databases.
    DatabasesDelete,
    /// List roles.
    RolesRead,
    /// Manage roles and privileges.
    RolesManage,
    /// Execute SQL.
    QueriesExecute,
    /// Read logs.
    LogsRead,
    /// Manage panel settings.
    SettingsManage,
    /// Manage updates.
    UpdatesManage,
    /// Read audit log.
    AuditRead,
    /// Manage panel users.
    UsersManage,
}

impl Permission {
    /// Stable string form.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClustersRead => "clusters.read",
            Self::ClustersCreate => "clusters.create",
            Self::ClustersControl => "clusters.control",
            Self::ClustersConfigure => "clusters.configure",
            Self::ClustersDelete => "clusters.delete",
            Self::DatabasesRead => "databases.read",
            Self::DatabasesCreate => "databases.create",
            Self::DatabasesDelete => "databases.delete",
            Self::RolesRead => "roles.read",
            Self::RolesManage => "roles.manage",
            Self::QueriesExecute => "queries.execute",
            Self::LogsRead => "logs.read",
            Self::SettingsManage => "settings.manage",
            Self::UpdatesManage => "updates.manage",
            Self::AuditRead => "audit.read",
            Self::UsersManage => "users.manage",
        }
    }
}

/// Permissions granted to a role.
pub fn permissions_for(role: Role) -> HashSet<Permission> {
    use Permission::*;
    let mut set = HashSet::new();
    match role {
        Role::Viewer => {
            set.extend([
                ClustersRead,
                DatabasesRead,
                RolesRead,
                LogsRead,
                AuditRead,
            ]);
        }
        Role::Operator => {
            set.extend(permissions_for(Role::Viewer));
            set.extend([
                ClustersControl,
                DatabasesCreate,
                DatabasesDelete,
                RolesManage,
                QueriesExecute,
            ]);
        }
        Role::Administrator => {
            set.extend(permissions_for(Role::Operator));
            set.extend([
                ClustersCreate,
                ClustersConfigure,
                ClustersDelete,
                SettingsManage,
                UpdatesManage,
                UsersManage,
            ]);
        }
        Role::Owner => {
            set.extend(permissions_for(Role::Administrator));
        }
    }
    set
}

/// Check whether a role has a permission.
pub fn role_has(role: Role, permission: Permission) -> bool {
    permissions_for(role).contains(&permission)
}

/// Whether SQL execution should be forced read-only for this role.
pub fn queries_read_only(role: Role) -> bool {
    matches!(role, Role::Operator) // operators may be restricted; viewers cannot execute at all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewer_cannot_delete_clusters() {
        assert!(!role_has(Role::Viewer, Permission::ClustersDelete));
        assert!(role_has(Role::Viewer, Permission::ClustersRead));
        assert!(!role_has(Role::Viewer, Permission::QueriesExecute));
    }

    #[test]
    fn owner_has_all() {
        assert!(role_has(Role::Owner, Permission::UsersManage));
        assert!(role_has(Role::Owner, Permission::ClustersDelete));
        assert!(role_has(Role::Administrator, Permission::ClustersDelete));
        assert!(!role_has(Role::Operator, Permission::ClustersDelete));
    }
}
