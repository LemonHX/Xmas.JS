//! Permissions configuration for vsys
//!
//! This module provides fine-grained permission control for filesystem,
//! network, and environment access.

use std::path::{Path, PathBuf};

/// Black or white list for permission checking
#[derive(Debug, PartialEq, Clone, Hash, Eq)]
pub enum BlackOrWhiteList {
    /// Allow all except items in the list
    BlackList(Vec<String>),
    /// Deny all except items in the list (default: empty = deny all)
    WhiteList(Vec<String>),
}

impl Default for BlackOrWhiteList {
    fn default() -> Self {
        // Default to deny all (empty whitelist)
        Self::WhiteList(vec![])
    }
}

impl BlackOrWhiteList {
    /// Create a blacklist (allow all except listed)
    pub fn blacklist(items: Vec<String>) -> Self {
        Self::BlackList(items)
    }

    /// Create a whitelist (deny all except listed)
    pub fn whitelist(items: Vec<String>) -> Self {
        Self::WhiteList(items)
    }

    /// Allow all (empty blacklist)
    pub fn allow_all() -> Self {
        Self::BlackList(vec![])
    }

    /// Deny all (empty whitelist)
    pub fn deny_all() -> Self {
        Self::WhiteList(vec![])
    }

    /// Check if a path is allowed
    pub fn check_path(&self, path: &Path) -> bool {
        let canonical_path = match path.canonicalize() {
            Ok(p) => p,
            Err(_) => return false,
        };

        let (is_whitelist, items) = match self {
            BlackOrWhiteList::BlackList(items) => (false, items),
            BlackOrWhiteList::WhiteList(items) => (true, items),
        };

        // Separate pattern paths (ending with *) and normal paths
        let mut normal_paths = Vec::new();
        let mut pattern_paths = Vec::new();

        for item in items {
            if item.ends_with('*') {
                let pattern = &item[..item.len() - 1];
                if let Ok(p) = Path::new(pattern).canonicalize() {
                    pattern_paths.push(p);
                } else {
                    pattern_paths.push(PathBuf::from(pattern));
                }
            } else if let Ok(p) = Path::new(item).canonicalize() {
                normal_paths.push(p);
            } else {
                normal_paths.push(PathBuf::from(item));
            }
        }

        // Check pattern paths (directory prefixes)
        for pattern in &pattern_paths {
            if canonical_path.starts_with(pattern) {
                return is_whitelist;
            }
        }

        // Check exact paths
        let found = normal_paths.iter().any(|p| p == &canonical_path);

        if is_whitelist {
            found
        } else {
            !found
        }
    }

    /// Check if a host/URL is allowed
    pub fn check_host(&self, host: &str) -> bool {
        let (is_whitelist, items) = match self {
            BlackOrWhiteList::BlackList(items) => (false, items),
            BlackOrWhiteList::WhiteList(items) => (true, items),
        };

        // Check for wildcard patterns
        for item in items {
            if item.starts_with("*.") {
                // Wildcard subdomain match
                let suffix = &item[1..]; // ".example.com"
                if host.ends_with(suffix) || host == &item[2..] {
                    return is_whitelist;
                }
            } else if item == host {
                return is_whitelist;
            }
        }

        // No match found
        !is_whitelist
    }
}

/// Struct representing permissions for filesystem, network, and environment access.
///
/// **WARNING**: by default, no permissions are granted (all whitelists are empty).
#[derive(Debug, Clone, Default)]
pub struct Permissions {
    /// Filesystem access permissions
    pub fs: BlackOrWhiteList,
    /// Network access permissions
    pub net: BlackOrWhiteList,
    /// Environment variable access permissions
    pub env: BlackOrWhiteList,
    /// Standard I/O (console) access
    pub stdio: bool,
}

impl Permissions {
    /// Create permissions that allow everything
    pub fn allow_all() -> Self {
        Self {
            fs: BlackOrWhiteList::allow_all(),
            net: BlackOrWhiteList::allow_all(),
            env: BlackOrWhiteList::allow_all(),
            stdio: true,
        }
    }

    /// Create permissions that deny everything (default)
    pub fn deny_all() -> Self {
        Self::default()
    }

    /// Check if filesystem access to path is allowed
    pub fn check_fs(&self, path: &Path) -> bool {
        self.fs.check_path(path)
    }

    /// Check if network access to host is allowed
    pub fn check_net(&self, host: &str) -> bool {
        self.net.check_host(host)
    }

    /// Check if environment variable access is allowed
    pub fn check_env(&self, var_name: &str) -> bool {
        let (is_whitelist, items) = match &self.env {
            BlackOrWhiteList::BlackList(items) => (false, items),
            BlackOrWhiteList::WhiteList(items) => (true, items),
        };

        let found = items.iter().any(|item| item == var_name);

        if is_whitelist {
            found
        } else {
            !found
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_all_permissions() {
        let perm = Permissions::allow_all();
        assert!(perm.stdio);
        assert!(perm.check_net("example.com"));
        assert!(perm.check_env("PATH"));
    }

    #[test]
    fn test_deny_all_permissions() {
        let perm = Permissions::deny_all();
        assert!(!perm.stdio);
        assert!(!perm.check_net("example.com"));
        assert!(!perm.check_env("PATH"));
    }

    #[test]
    fn test_whitelist_net() {
        let perm = Permissions {
            net: BlackOrWhiteList::whitelist(vec!["api.example.com".to_string()]),
            ..Default::default()
        };
        assert!(perm.check_net("api.example.com"));
        assert!(!perm.check_net("other.com"));
    }

    #[test]
    fn test_blacklist_net() {
        let perm = Permissions {
            net: BlackOrWhiteList::blacklist(vec!["evil.com".to_string()]),
            ..Default::default()
        };
        assert!(!perm.check_net("evil.com"));
        assert!(perm.check_net("good.com"));
    }

    #[test]
    fn test_wildcard_net() {
        let perm = Permissions {
            net: BlackOrWhiteList::whitelist(vec!["*.example.com".to_string()]),
            ..Default::default()
        };
        assert!(perm.check_net("api.example.com"));
        assert!(perm.check_net("example.com"));
        assert!(!perm.check_net("other.com"));
    }
}
