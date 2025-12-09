use crate::types::{CredentialType, Result, SigilforgeError};
use std::fmt;

/// A parsed auth:// URI reference.
///
/// Format: `auth://{service}/{account}/{credential_type}`
///
/// Examples:
/// - `auth://spotify/personal/token`
/// - `auth://github/oss/api_key`
/// - `auth://openai/default/api_key`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AuthRef {
    /// The service identifier (e.g., "spotify", "github").
    pub service: String,
    /// The account identifier (e.g., "personal", "work").
    pub account: String,
    /// The type of credential being requested.
    pub credential_type: CredentialType,
}

impl AuthRef {
    /// Create a new auth reference.
    pub fn new(
        service: impl Into<String>,
        account: impl Into<String>,
        credential_type: CredentialType,
    ) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
            credential_type,
        }
    }

    /// Parse an auth:// URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use sigilforge_client::resolve::AuthRef;
    /// use sigilforge_client::types::CredentialType;
    ///
    /// let auth_ref = AuthRef::parse("auth://spotify/personal/token").unwrap();
    /// assert_eq!(auth_ref.service, "spotify");
    /// assert_eq!(auth_ref.account, "personal");
    /// assert_eq!(auth_ref.credential_type, CredentialType::Token);
    /// ```
    pub fn parse(uri: &str) -> Result<Self> {
        // Check scheme
        let rest = uri
            .strip_prefix("auth://")
            .ok_or_else(|| SigilforgeError::InvalidReference(
                format!("URI must start with 'auth://': {}", uri)
            ))?;

        // Split path components
        let parts: Vec<&str> = rest.split('/').collect();

        if parts.len() < 3 {
            return Err(SigilforgeError::InvalidReference(
                format!("URI must have format 'auth://service/account/type': {}", uri)
            ));
        }

        let service = parts[0];
        let account = parts[1];
        let cred_type_str = parts[2];

        if service.is_empty() {
            return Err(SigilforgeError::InvalidReference(
                "service cannot be empty".to_string()
            ));
        }

        if account.is_empty() {
            return Err(SigilforgeError::InvalidReference(
                "account cannot be empty".to_string()
            ));
        }

        let credential_type = cred_type_str.parse::<CredentialType>()
            .map_err(|_| SigilforgeError::InvalidReference(
                format!("unknown credential type: {}", cred_type_str)
            ))?;

        Ok(Self {
            service: service.to_string(),
            account: account.to_string(),
            credential_type,
        })
    }

    /// Convert to auth:// URI string.
    pub fn to_uri(&self) -> String {
        format!(
            "auth://{}/{}/{}",
            self.service, self.account, self.credential_type
        )
    }

    /// Convert to storage key format.
    ///
    /// This matches the key format used by Sigilforge's SecretStore.
    pub fn to_storage_key(&self) -> String {
        format!(
            "sigilforge/{}/{}/{}",
            self.service, self.account, self.credential_type
        )
    }

    /// Convert to environment variable name.
    ///
    /// Format: `SIGILFORGE_{SERVICE}_{ACCOUNT}_{TYPE}`
    ///
    /// Examples:
    /// - `auth://spotify/personal/token` -> `SIGILFORGE_SPOTIFY_PERSONAL_TOKEN`
    /// - `auth://github/oss/api_key` -> `SIGILFORGE_GITHUB_OSS_API_KEY`
    pub fn to_env_var(&self) -> String {
        format!(
            "SIGILFORGE_{}_{}_{}",
            self.service.to_uppercase(),
            self.account.to_uppercase(),
            self.credential_type.env_suffix()
        )
    }
}

impl fmt::Display for AuthRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uri())
    }
}

impl std::str::FromStr for AuthRef {
    type Err = SigilforgeError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

/// Check if a string looks like an auth:// reference.
pub fn is_auth_uri(s: &str) -> bool {
    s.starts_with("auth://")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_uris() {
        let cases = vec![
            ("auth://spotify/personal/token", "spotify", "personal", CredentialType::Token),
            ("auth://github/oss/api_key", "github", "oss", CredentialType::ApiKey),
            ("auth://openai/default/api_key", "openai", "default", CredentialType::ApiKey),
            ("auth://gmail/work/refresh_token", "gmail", "work", CredentialType::RefreshToken),
            ("auth://oauth/app/client_id", "oauth", "app", CredentialType::ClientId),
            ("auth://oauth/app/client_secret", "oauth", "app", CredentialType::ClientSecret),
        ];

        for (uri, expected_service, expected_account, expected_type) in cases {
            let auth_ref = AuthRef::parse(uri).unwrap();
            assert_eq!(auth_ref.service, expected_service, "service mismatch for {}", uri);
            assert_eq!(auth_ref.account, expected_account, "account mismatch for {}", uri);
            assert_eq!(auth_ref.credential_type, expected_type, "type mismatch for {}", uri);
        }
    }

    #[test]
    fn test_parse_invalid_uris() {
        let cases = vec![
            "http://spotify/personal/token",  // wrong scheme
            "auth://spotify/personal",         // missing type
            "auth://spotify",                  // missing account and type
            "auth:///personal/token",          // missing service
            "auth://spotify//token",           // missing account
            "auth://spotify/personal/unknown", // unknown type
        ];

        for uri in cases {
            assert!(AuthRef::parse(uri).is_err(), "expected error for {}", uri);
        }
    }

    #[test]
    fn test_to_uri() {
        let auth_ref = AuthRef::new("spotify", "personal", CredentialType::Token);
        assert_eq!(auth_ref.to_uri(), "auth://spotify/personal/token");
    }

    #[test]
    fn test_to_storage_key() {
        let auth_ref = AuthRef::new("spotify", "personal", CredentialType::Token);
        assert_eq!(auth_ref.to_storage_key(), "sigilforge/spotify/personal/token");
    }

    #[test]
    fn test_to_env_var() {
        let cases = vec![
            (AuthRef::new("spotify", "personal", CredentialType::Token), "SIGILFORGE_SPOTIFY_PERSONAL_TOKEN"),
            (AuthRef::new("github", "oss", CredentialType::ApiKey), "SIGILFORGE_GITHUB_OSS_API_KEY"),
            (AuthRef::new("gmail", "work", CredentialType::RefreshToken), "SIGILFORGE_GMAIL_WORK_REFRESH_TOKEN"),
        ];

        for (auth_ref, expected) in cases {
            assert_eq!(auth_ref.to_env_var(), expected);
        }
    }

    #[test]
    fn test_roundtrip() {
        let original = AuthRef::new("spotify", "personal", CredentialType::Token);
        let uri = original.to_uri();
        let parsed = AuthRef::parse(&uri).unwrap();
        assert_eq!(original, parsed);
    }
}
