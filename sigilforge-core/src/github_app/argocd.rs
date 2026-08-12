//! Rendering an Argo CD repository `Secret` from a GitHub App registration.
//!
//! Argo CD discovers repository credentials by watching `Secret`s in its own
//! namespace that carry the label `argocd.argoproj.io/secret-type: repository`.
//! For GitHub App auth it reads five keys: `type`, `url`, `githubAppID`,
//! `githubAppInstallationID`, and `githubAppPrivateKey`.
//!
//! This module renders exactly that manifest, taking the key material from
//! Sigilforge instead of from a file someone copied around.
//!
//! # The rendered manifest contains a private key
//!
//! It is meant to be **piped**, not saved:
//!
//! ```bash
//! sigilforge github-app argocd-secret raibid-labs \
//!     --repo-url https://github.com/raibid-labs/raibid-fish.git \
//!   | kubectl apply -f -
//! ```
//!
//! If it must be persisted, encrypt it first (SOPS, sealed-secrets, or the
//! equivalent). A plaintext copy in Git is the credential sprawl this whole
//! feature exists to avoid.

use super::{GitHubAppCredential, GitHubAppError};
use crate::store::Secret;

/// The Argo CD namespace repository secrets are read from by default.
pub const DEFAULT_ARGOCD_NAMESPACE: &str = "argocd";

/// The label Argo CD uses to recognise a repository credential secret.
pub const ARGOCD_REPOSITORY_LABEL: &str = "argocd.argoproj.io/secret-type";

/// Kubernetes caps a resource name at 253 characters.
const MAX_NAME_LEN: usize = 253;

/// An Argo CD repository credential `Secret`, ready to render.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "github-app")]
/// # {
/// use sigilforge_core::github_app::ArgoCdRepositorySecret;
/// use sigilforge_core::store::Secret;
///
/// let manifest = ArgoCdRepositorySecret::new(
///     "https://github.com/raibid-labs/raibid-fish.git",
///     1234567,
///     89012345,
/// )
/// .unwrap()
/// .render(&Secret::new("-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----\n"));
///
/// assert!(manifest.contains("name: repo-raibid-labs-raibid-fish"));
/// assert!(manifest.contains("argocd.argoproj.io/secret-type: repository"));
/// assert!(manifest.contains("githubAppID: \"1234567\""));
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgoCdRepositorySecret {
    name: String,
    namespace: String,
    repo_url: String,
    app_id: u64,
    installation_id: u64,
    project: Option<String>,
    enterprise_base_url: Option<String>,
}

impl ArgoCdRepositorySecret {
    /// Build a manifest description for a repository URL and App identity.
    ///
    /// The secret name defaults to [`default_secret_name`] of the URL.
    ///
    /// # Errors
    ///
    /// Returns [`GitHubAppError::InvalidRepoUrl`] if no `owner/repo` pair can be
    /// read out of `repo_url`.
    pub fn new(
        repo_url: impl Into<String>,
        app_id: u64,
        installation_id: u64,
    ) -> Result<Self, GitHubAppError> {
        let repo_url = repo_url.into();
        let name = default_secret_name(&repo_url)?;

        Ok(Self {
            name,
            namespace: DEFAULT_ARGOCD_NAMESPACE.to_string(),
            repo_url,
            app_id,
            installation_id,
            project: None,
            enterprise_base_url: None,
        })
    }

    /// Build from a stored registration, taking the App and installation IDs from it.
    pub fn from_credential(
        repo_url: impl Into<String>,
        credential: &GitHubAppCredential,
    ) -> Result<Self, GitHubAppError> {
        Self::new(repo_url, credential.app_id(), credential.installation_id())
    }

    /// Override the generated secret name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Override the namespace (default [`DEFAULT_ARGOCD_NAMESPACE`]).
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Scope the credential to a single Argo CD project.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Point at a GitHub Enterprise Server API root.
    pub fn with_enterprise_base_url(mut self, url: impl Into<String>) -> Self {
        self.enterprise_base_url = Some(url.into());
        self
    }

    /// The secret's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The secret's namespace.
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// The repository URL the credential applies to.
    pub fn repo_url(&self) -> &str {
        &self.repo_url
    }

    /// Render the manifest as YAML.
    ///
    /// The private key is emitted as a literal block scalar (`|`), which is the
    /// only YAML form that survives a multi-line PEM without escaping. Every
    /// other value is quoted, so a URL containing `:` or `#` cannot break the
    /// document.
    ///
    /// The output starts with a comment warning that it holds a private key.
    pub fn render(&self, private_key_pem: &Secret) -> String {
        let mut out = String::new();

        out.push_str("# Generated by sigilforge. CONTAINS A GITHUB APP PRIVATE KEY.\n");
        out.push_str(
            "# Pipe this to `kubectl apply -f -`, or encrypt it (SOPS / sealed-secrets)\n",
        );
        out.push_str("# before it touches disk. Do not commit it.\n");
        out.push_str("apiVersion: v1\n");
        out.push_str("kind: Secret\n");
        out.push_str("metadata:\n");
        out.push_str(&format!("  name: {}\n", self.name));
        out.push_str(&format!("  namespace: {}\n", self.namespace));
        out.push_str("  labels:\n");
        out.push_str(&format!("    {}: repository\n", ARGOCD_REPOSITORY_LABEL));
        out.push_str("type: Opaque\n");
        out.push_str("stringData:\n");
        out.push_str("  type: git\n");
        out.push_str(&format!("  url: {}\n", quote(&self.repo_url)));
        out.push_str(&format!("  githubAppID: \"{}\"\n", self.app_id));
        out.push_str(&format!(
            "  githubAppInstallationID: \"{}\"\n",
            self.installation_id
        ));

        if let Some(project) = &self.project {
            out.push_str(&format!("  project: {}\n", quote(project)));
        }

        if let Some(base_url) = &self.enterprise_base_url {
            out.push_str(&format!(
                "  githubAppEnterpriseBaseUrl: {}\n",
                quote(base_url)
            ));
        }

        out.push_str("  githubAppPrivateKey: |\n");
        for line in private_key_pem.expose().trim_end().lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                out.push_str("    ");
                out.push_str(line);
                out.push('\n');
            }
        }

        out
    }
}

/// Derive a Kubernetes-safe secret name from a repository URL.
///
/// `https://github.com/raibid-labs/raibid-fish.git` becomes
/// `repo-raibid-labs-raibid-fish`. Handles HTTPS, `ssh://`, and the scp-style
/// `git@host:owner/repo` form.
///
/// # Errors
///
/// Returns [`GitHubAppError::InvalidRepoUrl`] when no `owner/repo` pair is present.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "github-app")]
/// # {
/// use sigilforge_core::github_app::default_secret_name;
///
/// assert_eq!(
///     default_secret_name("https://github.com/raibid-labs/raibid-fish.git").unwrap(),
///     "repo-raibid-labs-raibid-fish"
/// );
/// assert_eq!(
///     default_secret_name("git@github.com:raibid-labs/spark-infra.git").unwrap(),
///     "repo-raibid-labs-spark-infra"
/// );
/// # }
/// ```
pub fn default_secret_name(repo_url: &str) -> Result<String, GitHubAppError> {
    let (owner, repo) =
        split_owner_repo(repo_url).ok_or_else(|| GitHubAppError::InvalidRepoUrl {
            url: repo_url.to_string(),
        })?;

    let name = sanitize_name(&format!("repo-{}-{}", owner, repo));

    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(GitHubAppError::InvalidRepoUrl {
            url: repo_url.to_string(),
        });
    }

    Ok(name)
}

/// Pull `owner` and `repo` out of the tail of a repository URL.
fn split_owner_repo(repo_url: &str) -> Option<(String, String)> {
    let trimmed = repo_url.trim();
    let without_git = trimmed.strip_suffix(".git").unwrap_or(trimmed);

    // Strip the scheme/host: `scheme://[user@]host/path`, `[user@]host:path`,
    // or a bare `owner/repo`.
    let path = if let Some((_, after_scheme)) = without_git.split_once("://") {
        after_scheme.split_once('/').map(|(_, path)| path)?
    } else if let Some((_, after_colon)) = without_git.split_once(':') {
        after_colon
    } else {
        without_git
    };

    let segments: Vec<&str> = path
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();

    if segments.len() < 2 {
        return None;
    }

    Some((
        segments[segments.len() - 2].to_string(),
        segments[segments.len() - 1].to_string(),
    ))
}

/// Reduce a string to a valid RFC 1123 DNS subdomain label sequence.
fn sanitize_name(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_was_dash = false;

    for ch in raw.to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_was_dash = false;
        } else if !last_was_dash {
            out.push('-');
            last_was_dash = true;
        }
    }

    out.trim_matches('-').to_string()
}

/// Double-quote a YAML scalar, escaping backslashes and quotes.
fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_app::test_support::TEST_PRIVATE_KEY_PEM;

    fn pem() -> Secret {
        Secret::new(TEST_PRIVATE_KEY_PEM)
    }

    fn manifest() -> String {
        ArgoCdRepositorySecret::new(
            "https://github.com/raibid-labs/raibid-fish.git",
            1234567,
            89012345,
        )
        .unwrap()
        .render(&pem())
    }

    #[test]
    fn test_default_secret_name_from_https_url() {
        assert_eq!(
            default_secret_name("https://github.com/raibid-labs/raibid-fish.git").unwrap(),
            "repo-raibid-labs-raibid-fish"
        );
    }

    #[test]
    fn test_default_secret_name_without_git_suffix() {
        assert_eq!(
            default_secret_name("https://github.com/raibid-labs/spark-infra").unwrap(),
            "repo-raibid-labs-spark-infra"
        );
    }

    #[test]
    fn test_default_secret_name_from_scp_style_url() {
        assert_eq!(
            default_secret_name("git@github.com:raibid-labs/spark-infra.git").unwrap(),
            "repo-raibid-labs-spark-infra"
        );
    }

    #[test]
    fn test_default_secret_name_from_ssh_url() {
        assert_eq!(
            default_secret_name("ssh://git@github.com/raibid-labs/raibid-fish.git").unwrap(),
            "repo-raibid-labs-raibid-fish"
        );
    }

    #[test]
    fn test_default_secret_name_lowercases_and_sanitizes() {
        assert_eq!(
            default_secret_name("https://github.com/Raibid_Labs/Raibid.Fish").unwrap(),
            "repo-raibid-labs-raibid-fish"
        );
    }

    #[test]
    fn test_default_secret_name_rejects_urls_without_owner() {
        let err = default_secret_name("https://github.com/raibid-fish").unwrap_err();
        assert!(matches!(err, GitHubAppError::InvalidRepoUrl { .. }));
    }

    #[test]
    fn test_default_secret_name_rejects_empty_input() {
        assert!(default_secret_name("").is_err());
    }

    #[test]
    fn test_manifest_has_argocd_repository_label() {
        assert!(manifest().contains("    argocd.argoproj.io/secret-type: repository\n"));
    }

    #[test]
    fn test_manifest_defaults_to_argocd_namespace() {
        assert!(manifest().contains("  namespace: argocd\n"));
    }

    #[test]
    fn test_manifest_carries_all_required_keys() {
        let rendered = manifest();
        for key in [
            "  type: git\n",
            "  url: \"https://github.com/raibid-labs/raibid-fish.git\"\n",
            "  githubAppID: \"1234567\"\n",
            "  githubAppInstallationID: \"89012345\"\n",
            "  githubAppPrivateKey: |\n",
        ] {
            assert!(
                rendered.contains(key),
                "missing {:?} in:\n{}",
                key,
                rendered
            );
        }
    }

    #[test]
    fn test_manifest_indents_every_private_key_line() {
        let rendered = manifest();
        let body = rendered.split("githubAppPrivateKey: |\n").nth(1).unwrap();

        assert!(body.starts_with("    -----BEGIN RSA PRIVATE KEY-----\n"));
        for line in body.lines().filter(|line| !line.is_empty()) {
            assert!(line.starts_with("    "), "unindented line: {:?}", line);
        }
        assert!(body.contains("    -----END RSA PRIVATE KEY-----\n"));
    }

    #[test]
    fn test_manifest_warns_that_it_holds_a_private_key() {
        let rendered = manifest();
        assert!(rendered.starts_with('#'));
        assert!(rendered.contains("PRIVATE KEY"));
        assert!(rendered.contains("Do not commit"));
    }

    #[test]
    fn test_manifest_omits_optional_fields_by_default() {
        let rendered = manifest();
        assert!(!rendered.contains("project:"));
        assert!(!rendered.contains("githubAppEnterpriseBaseUrl:"));
    }

    #[test]
    fn test_manifest_includes_project_when_set() {
        let rendered = ArgoCdRepositorySecret::new("https://github.com/o/r.git", 1, 2)
            .unwrap()
            .with_project("raibid")
            .render(&pem());

        assert!(rendered.contains("  project: \"raibid\"\n"));
    }

    #[test]
    fn test_manifest_includes_enterprise_base_url_when_set() {
        let rendered = ArgoCdRepositorySecret::new("https://ghe.example.com/o/r.git", 1, 2)
            .unwrap()
            .with_enterprise_base_url("https://ghe.example.com/api/v3")
            .render(&pem());

        assert!(
            rendered.contains("  githubAppEnterpriseBaseUrl: \"https://ghe.example.com/api/v3\"\n")
        );
    }

    #[test]
    fn test_manifest_honours_name_and_namespace_overrides() {
        let rendered = ArgoCdRepositorySecret::new("https://github.com/o/r.git", 1, 2)
            .unwrap()
            .with_name("custom-repo-secret")
            .with_namespace("gitops")
            .render(&pem());

        assert!(rendered.contains("  name: custom-repo-secret\n"));
        assert!(rendered.contains("  namespace: gitops\n"));
    }

    #[test]
    fn test_manifest_quotes_urls_containing_yaml_metacharacters() {
        let rendered = ArgoCdRepositorySecret::new("https://github.com/o/r.git", 1, 2)
            .unwrap()
            .with_project("has \"quotes\" and \\ backslash")
            .render(&pem());

        assert!(rendered.contains(r#"  project: "has \"quotes\" and \\ backslash""#));
    }

    #[test]
    fn test_from_credential_takes_ids_from_the_registration() {
        let credential = GitHubAppCredential::new(555, 666, TEST_PRIVATE_KEY_PEM).unwrap();
        let secret =
            ArgoCdRepositorySecret::from_credential("https://github.com/o/r.git", &credential)
                .unwrap();

        let rendered = secret.render(credential.private_key_pem());
        assert!(rendered.contains("githubAppID: \"555\""));
        assert!(rendered.contains("githubAppInstallationID: \"666\""));
    }

    #[test]
    fn test_accessors() {
        let secret =
            ArgoCdRepositorySecret::new("https://github.com/raibid-labs/raibid-fish.git", 1, 2)
                .unwrap();

        assert_eq!(secret.name(), "repo-raibid-labs-raibid-fish");
        assert_eq!(secret.namespace(), "argocd");
        assert_eq!(
            secret.repo_url(),
            "https://github.com/raibid-labs/raibid-fish.git"
        );
    }
}
