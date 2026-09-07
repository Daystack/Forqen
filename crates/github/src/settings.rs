//! Repository settings, read-only.
//!
//! Deliberately read-only. Changing branch protection or collaborator access
//! from a git client is a rare, consequential act that belongs where its
//! consequences are spelled out — and a client that offers it has to reproduce
//! GitHub's permission model correctly or it will show buttons that fail.
//! Showing the current state answers the question people actually have: "why
//! was my push rejected".

use serde::{Deserialize, Serialize};

use crate::{Client, GhError, Response};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RepoSettings {
    pub full_name: String,
    pub description: Option<String>,
    pub private: bool,
    pub archived: bool,
    pub default_branch: Option<String>,
    pub visibility: Option<String>,
    /// What the signed-in user may do here. Absent when the token cannot see
    /// it, which is not the same as having no access.
    pub permissions: Option<Permissions>,
    pub license: Option<License>,
    pub open_issues_count: Option<u32>,
    pub forks_count: Option<u32>,
    pub stargazers_count: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Permissions {
    #[serde(default)]
    pub admin: bool,
    #[serde(default)]
    pub push: bool,
    #[serde(default)]
    pub pull: bool,
}

impl Permissions {
    /// The highest role these permissions amount to.
    pub fn role(&self) -> &'static str {
        if self.admin {
            "admin"
        } else if self.push {
            "write"
        } else if self.pull {
            "read"
        } else {
            "none"
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct License {
    pub spdx_id: Option<String>,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Collaborator {
    pub login: String,
    pub permissions: Option<Permissions>,
}

/// Branch protection, as far as the token can see it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Protection {
    pub branch: String,
    /// `None` when the endpoint answered 404 — either the branch is
    /// unprotected or the token cannot read protection, and GitHub does not
    /// distinguish them. Saying "unknown" beats claiming "unprotected".
    pub protected: Option<bool>,
    pub required_reviews: Option<u32>,
    pub required_checks: Vec<String>,
    pub enforces_admins: bool,
}

#[derive(Deserialize)]
struct ProtectionRaw {
    #[serde(rename = "required_pull_request_reviews")]
    reviews: Option<ReviewsRaw>,
    #[serde(rename = "required_status_checks")]
    checks: Option<ChecksRaw>,
    #[serde(rename = "enforce_admins")]
    admins: Option<EnabledRaw>,
}

#[derive(Deserialize)]
struct ReviewsRaw {
    #[serde(rename = "required_approving_review_count")]
    count: Option<u32>,
}

#[derive(Deserialize)]
struct ChecksRaw {
    #[serde(default)]
    contexts: Vec<String>,
}

#[derive(Deserialize)]
struct EnabledRaw {
    #[serde(default)]
    enabled: bool,
}

impl Client {
    pub async fn repo_settings(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Response<RepoSettings>, GhError> {
        self.get(&format!("/repos/{owner}/{repo}")).await
    }

    /// Collaborators. Requires push access; a read-only token gets 403.
    pub async fn collaborators(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Vec<Collaborator>, GhError> {
        match self
            .get::<Vec<Collaborator>>(&format!("/repos/{owner}/{repo}/collaborators?per_page=50"))
            .await
        {
            Ok(r) => Ok(r.data),
            // 403 means "you may not list these", which is a fact about the
            // token rather than a failure worth surfacing as an error.
            Err(GhError::Api { status: 403, .. }) => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Protection on one branch.
    pub async fn branch_protection(
        &self,
        owner: &str,
        repo: &str,
        branch: &str,
    ) -> Result<Protection, GhError> {
        let path = format!("/repos/{owner}/{repo}/branches/{branch}/protection");
        match self.get::<ProtectionRaw>(&path).await {
            Ok(r) => Ok(Protection {
                branch: branch.to_string(),
                protected: Some(true),
                required_reviews: r.data.reviews.and_then(|x| x.count),
                required_checks: r.data.checks.map(|c| c.contexts).unwrap_or_default(),
                enforces_admins: r.data.admins.map(|a| a.enabled).unwrap_or(false),
            }),
            // 404 here is ambiguous: no protection, or no permission to read
            // it. Reporting "unknown" is honest; reporting "unprotected" would
            // tell someone their main branch is open when it may not be.
            Err(GhError::Api { status: 404, .. }) | Err(GhError::Api { status: 403, .. }) => {
                Ok(Protection {
                    branch: branch.to_string(),
                    protected: None,
                    ..Default::default()
                })
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permissions_collapse_to_the_highest_role() {
        let mk = |a, p, r| Permissions {
            admin: a,
            push: p,
            pull: r,
        };
        // GitHub sets the lower flags too, so order of checks matters.
        assert_eq!(mk(true, true, true).role(), "admin");
        assert_eq!(mk(false, true, true).role(), "write");
        assert_eq!(mk(false, false, true).role(), "read");
        assert_eq!(mk(false, false, false).role(), "none");
    }

    #[test]
    fn a_repository_payload_parses_with_only_its_core_fields() {
        let r: RepoSettings =
            serde_json::from_str(r#"{"full_name":"o/r","private":false,"archived":false}"#)
                .unwrap();
        assert_eq!(r.full_name, "o/r");
        assert_eq!(r.permissions, None, "absent is not the same as no access");
        assert_eq!(r.default_branch, None);
        assert_eq!(r.license, None);
    }

    #[test]
    fn a_full_payload_reads_permissions_and_licence() {
        let r: RepoSettings = serde_json::from_str(
            r#"{"full_name":"n1th1n-19/Forgen","private":true,"archived":false,
                "default_branch":"main","visibility":"private",
                "permissions":{"admin":true,"push":true,"pull":true},
                "license":{"spdx_id":"GPL-3.0","name":"GNU General Public License v3.0"},
                "open_issues_count":3,"stargazers_count":7}"#,
        )
        .unwrap();
        assert_eq!(r.permissions.unwrap().role(), "admin");
        assert_eq!(r.license.unwrap().spdx_id.as_deref(), Some("GPL-3.0"));
        assert_eq!(r.default_branch.as_deref(), Some("main"));
        assert!(r.private);
    }

    #[test]
    fn protection_details_are_read_when_present() {
        let raw: ProtectionRaw = serde_json::from_str(
            r#"{"required_pull_request_reviews":{"required_approving_review_count":2},
                "required_status_checks":{"contexts":["ci/engine","ci/ui"]},
                "enforce_admins":{"enabled":true}}"#,
        )
        .unwrap();

        let p = Protection {
            branch: "main".into(),
            protected: Some(true),
            required_reviews: raw.reviews.and_then(|x| x.count),
            required_checks: raw.checks.map(|c| c.contexts).unwrap_or_default(),
            enforces_admins: raw.admins.map(|a| a.enabled).unwrap_or(false),
        };

        assert_eq!(p.required_reviews, Some(2));
        assert_eq!(p.required_checks, ["ci/engine", "ci/ui"]);
        assert!(p.enforces_admins);
    }

    #[test]
    fn a_protection_payload_with_nothing_set_is_still_protected() {
        // A branch can be protected with no rules attached; that is different
        // from being unprotected.
        let raw: ProtectionRaw = serde_json::from_str("{}").unwrap();
        let p = Protection {
            branch: "main".into(),
            protected: Some(true),
            required_reviews: raw.reviews.and_then(|x| x.count),
            required_checks: raw.checks.map(|c| c.contexts).unwrap_or_default(),
            enforces_admins: raw.admins.map(|a| a.enabled).unwrap_or(false),
        };
        assert_eq!(p.protected, Some(true));
        assert_eq!(p.required_reviews, None);
        assert!(p.required_checks.is_empty());
    }

    #[test]
    fn unknown_protection_is_distinct_from_unprotected() {
        // 404 means "no protection, or you may not read it" — GitHub does not
        // distinguish. Claiming "unprotected" could tell someone their main
        // branch is open when it is not.
        let unknown = Protection {
            branch: "main".into(),
            protected: None,
            ..Default::default()
        };
        assert_eq!(unknown.protected, None);
        assert_ne!(unknown.protected, Some(false));
    }
}
