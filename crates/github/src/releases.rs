//! Releases and their assets.
//!
//! A release is a tag plus notes plus files. The tag part is local git; this
//! covers the GitHub half — the notes people actually read and the binaries
//! they actually download.

use serde::{Deserialize, Serialize};

use crate::{Client, GhError, Response};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Release {
    pub id: u64,
    pub tag_name: String,
    /// Display name. Frequently empty, in which case the tag is the title.
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub created_at: Option<String>,
    pub published_at: Option<String>,
    pub html_url: Option<String>,
    pub author: Option<crate::models::User>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    /// What to show as the heading.
    ///
    /// GitHub allows an empty name and most tooling-generated releases have
    /// one, so falling back to the tag is the common path rather than an edge
    /// case.
    pub fn title(&self) -> &str {
        match self.name.as_deref() {
            Some(n) if !n.trim().is_empty() => n,
            _ => &self.tag_name,
        }
    }

    /// One word for the state.
    ///
    /// Draft and prerelease are independent flags — a draft prerelease is
    /// legal — so they cannot be read as an enum, and "draft" is the one that
    /// matters more since a draft is invisible to everyone else.
    pub fn state(&self) -> &'static str {
        match (self.draft, self.prerelease) {
            (true, _) => "draft",
            (false, true) => "prerelease",
            (false, false) => "released",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Asset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub download_count: u64,
    pub browser_download_url: Option<String>,
}

impl Asset {
    /// Human-readable size.
    ///
    /// Binary units, because that is what a file manager shows and a release
    /// asset is a file — a number that disagrees with the desktop by 7% reads
    /// as a bug.
    pub fn human_size(&self) -> String {
        const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
        let mut size = self.size as f64;
        let mut unit = 0;
        while size >= 1024.0 && unit < UNITS.len() - 1 {
            size /= 1024.0;
            unit += 1;
        }
        if unit == 0 {
            format!("{} {}", self.size, UNITS[0])
        } else {
            format!("{size:.1} {}", UNITS[unit])
        }
    }
}

/// What to create.
///
/// A struct rather than five positional parameters: two of them are bools, and
/// `create_release(.., true, false)` is one transposition away from publishing
/// a draft to everyone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewRelease {
    pub tag: String,
    pub name: String,
    pub body: String,
    pub draft: bool,
    pub prerelease: bool,
}

impl Client {
    /// Releases, newest first. Drafts are included when the token can see them.
    pub async fn releases(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<Response<Vec<Release>>, GhError> {
        self.get(&format!("/repos/{owner}/{repo}/releases?per_page=30"))
            .await
    }

    /// Create a release for an existing tag.
    ///
    /// Deliberately does not create the tag: a release pointing at a tag that
    /// was never pushed is a broken link, and pushing a tag is a git operation
    /// the user should make deliberately.
    pub async fn create_release(
        &self,
        owner: &str,
        repo: &str,
        new: &NewRelease,
    ) -> Result<(), GhError> {
        let NewRelease {
            tag,
            name,
            body,
            draft,
            prerelease,
        } = new;

        if tag.trim().is_empty() {
            return Err(GhError::Api {
                status: 422,
                message: "a release needs a tag".into(),
            });
        }
        self.post_no_content(
            &format!("/repos/{owner}/{repo}/releases"),
            serde_json::json!({
                "tag_name": tag,
                "name": name,
                "body": body,
                "draft": draft,
                "prerelease": prerelease,
            }),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_with_no_name_shows_its_tag() {
        // Tooling-generated releases usually have an empty name; this is the
        // common path, not an edge case.
        let r: Release = serde_json::from_str(
            r#"{"id":1,"tag_name":"v1.2.0","name":null,"draft":false,"prerelease":false}"#,
        )
        .unwrap();
        assert_eq!(r.title(), "v1.2.0");

        let blank: Release = serde_json::from_str(
            r#"{"id":1,"tag_name":"v1.2.0","name":"   ","draft":false,"prerelease":false}"#,
        )
        .unwrap();
        assert_eq!(blank.title(), "v1.2.0", "whitespace is not a name");
    }

    #[test]
    fn a_named_release_shows_its_name() {
        let r: Release = serde_json::from_str(
            r#"{"id":1,"tag_name":"v1.2.0","name":"Bug fixes","draft":false,"prerelease":false}"#,
        )
        .unwrap();
        assert_eq!(r.title(), "Bug fixes");
    }

    #[test]
    fn draft_and_prerelease_are_independent_flags() {
        let mk = |d: bool, p: bool| Release {
            id: 1,
            tag_name: "v1".into(),
            name: None,
            body: None,
            draft: d,
            prerelease: p,
            created_at: None,
            published_at: None,
            html_url: None,
            author: None,
            assets: vec![],
        };
        assert_eq!(mk(false, false).state(), "released");
        assert_eq!(mk(false, true).state(), "prerelease");
        assert_eq!(mk(true, false).state(), "draft");
        assert_eq!(
            mk(true, true).state(),
            "draft",
            "a draft prerelease is legal, and draft is the fact that matters"
        );
    }

    #[test]
    fn asset_sizes_use_binary_units_like_the_file_manager() {
        let mk = |size: u64| Asset {
            id: 1,
            name: "x".into(),
            size,
            download_count: 0,
            browser_download_url: None,
        };
        assert_eq!(mk(0).human_size(), "0 B");
        assert_eq!(mk(512).human_size(), "512 B");
        assert_eq!(mk(1024).human_size(), "1.0 KiB");
        assert_eq!(mk(1_048_576).human_size(), "1.0 MiB");
        assert_eq!(mk(8_937_200).human_size(), "8.5 MiB");
    }

    #[test]
    fn a_release_with_assets_parses() {
        let r: Release = serde_json::from_str(
            r#"{"id":1,"tag_name":"v1","draft":false,"prerelease":false,
                "assets":[{"id":9,"name":"forqen.flatpak","size":8937200,
                           "download_count":42,
                           "browser_download_url":"https://x/forqen.flatpak"}]}"#,
        )
        .unwrap();
        assert_eq!(r.assets.len(), 1);
        assert_eq!(r.assets[0].human_size(), "8.5 MiB");
        assert_eq!(r.assets[0].download_count, 42);
    }

    #[test]
    fn a_release_with_no_assets_defaults_to_empty() {
        let r: Release =
            serde_json::from_str(r#"{"id":1,"tag_name":"v1","draft":true,"prerelease":false}"#)
                .unwrap();
        assert!(r.assets.is_empty());
        assert_eq!(r.state(), "draft");
    }
}
