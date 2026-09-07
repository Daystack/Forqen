//! Gists.
//!
//! A gist is a repository, but nobody treats it as one — it is a paste with a
//! URL. So this covers the two things people do: look at the ones they have,
//! and make a new one from a file or a selection.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Client, GhError, Response};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Gist {
    pub id: String,
    pub description: Option<String>,
    pub public: bool,
    pub html_url: Option<String>,
    pub created_at: Option<String>,
    /// Keyed by filename. A `BTreeMap` rather than a `HashMap` so the order is
    /// the same on every render — a list that reshuffles between refreshes
    /// looks like it changed when it did not.
    #[serde(default)]
    pub files: BTreeMap<String, GistFile>,
}

impl Gist {
    /// What to show as the title.
    ///
    /// A gist's description is optional and frequently empty, in which case
    /// GitHub's own UI falls back to the first filename.
    pub fn title(&self) -> String {
        match self.description.as_deref() {
            Some(d) if !d.trim().is_empty() => d.to_string(),
            _ => self
                .files
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| self.id.clone()),
        }
    }

    pub fn visibility(&self) -> &'static str {
        // "secret", not "private": a secret gist is reachable by anyone with
        // the URL, and calling it private would overstate the protection.
        if self.public {
            "public"
        } else {
            "secret"
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct GistFile {
    pub filename: String,
    pub size: Option<u64>,
    pub language: Option<String>,
    /// Absent in list responses; present when a single gist is fetched.
    pub content: Option<String>,
}

/// A gist to create.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewGist {
    pub description: String,
    pub filename: String,
    pub content: String,
    pub public: bool,
}

impl Client {
    /// The signed-in user's gists, newest first.
    pub async fn gists(&self) -> Result<Response<Vec<Gist>>, GhError> {
        self.get("/gists?per_page=50").await
    }

    /// One gist, with file contents.
    pub async fn gist(&self, id: &str) -> Result<Response<Gist>, GhError> {
        self.get(&format!("/gists/{id}")).await
    }

    pub async fn create_gist(&self, new: &NewGist) -> Result<(), GhError> {
        if new.content.trim().is_empty() {
            return Err(GhError::Api {
                status: 422,
                message: "an empty gist has nothing to share".into(),
            });
        }

        // GitHub derives the language from the extension, so a filename with
        // none produces an unhighlighted paste. Not an error, but worth a
        // sensible default rather than an empty name, which is rejected.
        let filename = if new.filename.trim().is_empty() {
            "gistfile1.txt"
        } else {
            new.filename.trim()
        };

        self.post_no_content(
            "/gists",
            serde_json::json!({
                "description": new.description,
                "public": new.public,
                "files": { filename: { "content": new.content } },
            }),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Gist {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_described_gist_shows_its_description() {
        let g = parse(
            r#"{"id":"abc","description":"A useful snippet","public":true,
                "files":{"a.rs":{"filename":"a.rs"}}}"#,
        );
        assert_eq!(g.title(), "A useful snippet");
        assert_eq!(g.visibility(), "public");
    }

    #[test]
    fn a_gist_with_no_description_falls_back_to_its_first_filename() {
        // The common case: most gists are created without one.
        let g = parse(
            r#"{"id":"abc","description":"","public":false,
                "files":{"zeta.rs":{"filename":"zeta.rs"},
                         "alpha.rs":{"filename":"alpha.rs"}}}"#,
        );
        assert_eq!(
            g.title(),
            "alpha.rs",
            "BTreeMap orders the files, so the fallback is stable across renders"
        );
        assert_eq!(g.visibility(), "secret");
    }

    #[test]
    fn a_gist_with_neither_falls_back_to_its_id() {
        let g = parse(r#"{"id":"deadbeef","description":null,"public":true}"#);
        assert_eq!(g.title(), "deadbeef");
        assert!(g.files.is_empty());
    }

    #[test]
    fn a_non_public_gist_is_called_secret_not_private() {
        // Anyone with the URL can read it; "private" would overstate it.
        let g = parse(r#"{"id":"x","description":"d","public":false}"#);
        assert_eq!(g.visibility(), "secret");
    }

    #[test]
    fn file_order_is_stable_between_parses() {
        let json = r#"{"id":"x","description":"","public":true,
            "files":{"c.rs":{"filename":"c.rs"},
                     "a.rs":{"filename":"a.rs"},
                     "b.rs":{"filename":"b.rs"}}}"#;
        let first: Vec<String> = parse(json).files.keys().cloned().collect();
        let second: Vec<String> = parse(json).files.keys().cloned().collect();
        assert_eq!(first, ["a.rs", "b.rs", "c.rs"]);
        assert_eq!(first, second);
    }

    #[test]
    fn contents_are_present_on_a_single_gist_and_absent_from_a_list() {
        let listed = parse(r#"{"id":"x","public":true,"files":{"a.rs":{"filename":"a.rs"}}}"#);
        assert_eq!(listed.files["a.rs"].content, None);

        let single = parse(
            r#"{"id":"x","public":true,
                "files":{"a.rs":{"filename":"a.rs","content":"fn main() {}"}}}"#,
        );
        assert_eq!(
            single.files["a.rs"].content.as_deref(),
            Some("fn main() {}")
        );
    }
}
