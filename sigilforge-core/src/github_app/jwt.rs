//! RS256 JWT construction for GitHub App authentication.
//!
//! GitHub requires an App to present a JWT signed with its private key before it
//! will mint an installation token. The rules, from GitHub's documentation:
//!
//! - `alg` is `RS256`, `typ` is `JWT`
//! - `iss` is the App ID
//! - `iat` is the issue time, backdated ~60 seconds to tolerate clock drift
//!   between this machine and GitHub
//! - `exp` is at most **10 minutes** after `iat`; GitHub rejects anything longer
//!
//! This JWT is not the credential consumers use. It is only good for calling the
//! `/app/*` endpoints, and its whole job is to be exchanged for an installation
//! token (see [`super::token`]).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use rsa::signature::{SignatureEncoding, Signer};
use serde::{Deserialize, Serialize};

use super::{GitHubAppCredential, GitHubAppError};
use crate::store::Secret;

/// The longest lifetime GitHub accepts for an App JWT, in seconds.
pub const MAX_JWT_TTL_SECS: i64 = 600;

/// Default JWT lifetime, in seconds.
///
/// Nine minutes rather than the permitted ten: the last minute is headroom for
/// the round trip, so a JWT accepted at build time is still valid on arrival.
pub const DEFAULT_JWT_TTL_SECS: i64 = 540;

/// Seconds by which `iat` is backdated to absorb clock skew.
///
/// GitHub rejects a JWT whose `iat` is in the future relative to *their* clock,
/// so a local clock running slightly fast would otherwise break minting outright.
pub const CLOCK_SKEW_SECS: i64 = 60;

/// The JOSE header for a GitHub App JWT.
///
/// Field order is the serialized order, which is what gets signed - so it is
/// fixed here rather than left to a map's iteration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct JwtHeader {
    alg: &'static str,
    typ: &'static str,
}

impl Default for JwtHeader {
    fn default() -> Self {
        Self {
            alg: "RS256",
            typ: "JWT",
        }
    }
}

/// The claims of a GitHub App JWT.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "github-app")]
/// # {
/// use chrono::{Duration, TimeZone, Utc};
/// use sigilforge_core::github_app::AppJwtClaims;
///
/// let now = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
/// let claims = AppJwtClaims::new(1234567, now, Duration::minutes(9));
///
/// // `iat` is backdated 60s; `exp` is measured from the backdated `iat`.
/// assert_eq!(claims.iat, now.timestamp() - 60);
/// assert_eq!(claims.exp, claims.iat + 540);
/// assert_eq!(claims.iss, "1234567");
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppJwtClaims {
    /// Issued-at time, as a Unix timestamp, backdated by [`CLOCK_SKEW_SECS`].
    pub iat: i64,

    /// Expiry time, as a Unix timestamp. Never more than [`MAX_JWT_TTL_SECS`]
    /// after [`iat`](AppJwtClaims::iat).
    pub exp: i64,

    /// Issuer: the GitHub App ID, as a string.
    pub iss: String,
}

impl AppJwtClaims {
    /// Build claims for an App at a given instant.
    ///
    /// `ttl` is clamped to `1..=`[`MAX_JWT_TTL_SECS`] seconds - a caller asking
    /// for an hour gets ten minutes rather than a JWT GitHub will reject.
    pub fn new(app_id: u64, now: DateTime<Utc>, ttl: Duration) -> Self {
        let iat = now.timestamp() - CLOCK_SKEW_SECS;
        let ttl_secs = ttl.num_seconds().clamp(1, MAX_JWT_TTL_SECS);

        Self {
            iat,
            exp: iat + ttl_secs,
            iss: app_id.to_string(),
        }
    }

    /// The claim lifetime in seconds (`exp - iat`).
    pub fn ttl_secs(&self) -> i64 {
        self.exp - self.iat
    }
}

/// Build and sign an RS256 JWT for a GitHub App.
///
/// Returns the compact-serialized JWT (`header.payload.signature`) wrapped in a
/// [`Secret`], because a valid App JWT can mint installation tokens and is
/// therefore itself a credential.
///
/// # Errors
///
/// - [`GitHubAppError::InvalidPrivateKey`] if the stored PEM will not parse
/// - [`GitHubAppError::JwtSigning`] if serialization or the signature fails
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "github-app")]
/// # {
/// use chrono::{Duration, Utc};
/// use sigilforge_core::github_app::{GitHubAppCredential, sign_app_jwt};
///
/// let pem = sigilforge_core::github_app::test_support::TEST_PRIVATE_KEY_PEM;
/// let credential = GitHubAppCredential::new(1234567, 89012345, pem).unwrap();
///
/// let jwt = sign_app_jwt(&credential, Utc::now(), Duration::minutes(9)).unwrap();
/// assert_eq!(jwt.expose().split('.').count(), 3);
/// # }
/// ```
pub fn sign_app_jwt(
    credential: &GitHubAppCredential,
    now: DateTime<Utc>,
    ttl: Duration,
) -> Result<Secret, GitHubAppError> {
    let claims = AppJwtClaims::new(credential.app_id(), now, ttl);
    let signing_key = credential.signing_key()?;

    let signing_input = signing_input(&JwtHeader::default(), &claims)?;

    let signature = signing_key
        .try_sign(signing_input.as_bytes())
        .map_err(|e| GitHubAppError::JwtSigning {
            message: format!("RS256 signature failed: {}", e),
        })?;

    let encoded_signature = URL_SAFE_NO_PAD.encode(signature.to_bytes());

    Ok(Secret::new(format!(
        "{}.{}",
        signing_input, encoded_signature
    )))
}

/// Produce the `base64url(header).base64url(claims)` string that gets signed.
fn signing_input(header: &JwtHeader, claims: &AppJwtClaims) -> Result<String, GitHubAppError> {
    let header_json = serde_json::to_vec(header).map_err(|e| GitHubAppError::JwtSigning {
        message: format!("could not serialize JWT header: {}", e),
    })?;
    let claims_json = serde_json::to_vec(claims).map_err(|e| GitHubAppError::JwtSigning {
        message: format!("could not serialize JWT claims: {}", e),
    })?;

    Ok(format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header_json),
        URL_SAFE_NO_PAD.encode(claims_json)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github_app::test_support::TEST_PRIVATE_KEY_PEM;
    use chrono::TimeZone;
    use rsa::RsaPrivateKey;
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs1v15::VerifyingKey;
    use rsa::sha2::Sha256;
    use rsa::signature::Verifier;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 14, 15, 9, 26).unwrap()
    }

    fn credential() -> GitHubAppCredential {
        GitHubAppCredential::new(1234567, 89012345, TEST_PRIVATE_KEY_PEM).unwrap()
    }

    /// Decode a base64url JWT segment into JSON.
    fn decode_segment(segment: &str) -> serde_json::Value {
        let bytes = URL_SAFE_NO_PAD.decode(segment).unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn test_claims_backdate_iat_by_clock_skew() {
        let now = fixed_now();
        let claims = AppJwtClaims::new(42, now, Duration::minutes(9));

        assert_eq!(claims.iat, now.timestamp() - CLOCK_SKEW_SECS);
    }

    #[test]
    fn test_claims_exp_is_ttl_after_iat() {
        let claims = AppJwtClaims::new(42, fixed_now(), Duration::minutes(9));

        assert_eq!(claims.ttl_secs(), 540);
        assert_eq!(claims.exp, claims.iat + 540);
    }

    #[test]
    fn test_claims_iss_is_app_id_as_string() {
        let claims = AppJwtClaims::new(1234567, fixed_now(), Duration::minutes(9));
        assert_eq!(claims.iss, "1234567");
    }

    #[test]
    fn test_claims_ttl_clamped_to_ten_minutes() {
        // GitHub rejects anything past 10 minutes, so an over-long request is
        // clamped rather than passed through to be rejected at the API.
        let claims = AppJwtClaims::new(42, fixed_now(), Duration::hours(1));

        assert_eq!(claims.ttl_secs(), MAX_JWT_TTL_SECS);
        assert_eq!(claims.exp, claims.iat + MAX_JWT_TTL_SECS);
    }

    #[test]
    fn test_claims_ttl_clamped_to_at_least_one_second() {
        let claims = AppJwtClaims::new(42, fixed_now(), Duration::seconds(-30));
        assert_eq!(claims.ttl_secs(), 1);
    }

    #[test]
    fn test_claims_default_ttl_is_under_the_limit() {
        // Headroom for the round trip: a JWT built at the ceiling can arrive
        // already rejected.
        const _: () = assert!(DEFAULT_JWT_TTL_SECS < MAX_JWT_TTL_SECS);
        assert_eq!(DEFAULT_JWT_TTL_SECS, 540);
    }

    #[test]
    fn test_jwt_has_three_segments() {
        let jwt = sign_app_jwt(&credential(), fixed_now(), Duration::minutes(9)).unwrap();
        assert_eq!(jwt.expose().split('.').count(), 3);
    }

    #[test]
    fn test_jwt_header_declares_rs256() {
        let jwt = sign_app_jwt(&credential(), fixed_now(), Duration::minutes(9)).unwrap();
        let jwt = jwt.expose().to_string();
        let header = decode_segment(jwt.split('.').next().unwrap());

        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["typ"], "JWT");
    }

    #[test]
    fn test_jwt_payload_carries_expected_claims() {
        let now = fixed_now();
        let jwt = sign_app_jwt(&credential(), now, Duration::minutes(9)).unwrap();
        let jwt = jwt.expose().to_string();
        let payload = decode_segment(jwt.split('.').nth(1).unwrap());

        assert_eq!(payload["iss"], "1234567");
        assert_eq!(payload["iat"], now.timestamp() - CLOCK_SKEW_SECS);
        assert_eq!(payload["exp"], now.timestamp() - CLOCK_SKEW_SECS + 540);
    }

    #[test]
    fn test_jwt_signature_verifies_against_the_public_key() {
        let now = fixed_now();
        let jwt = sign_app_jwt(&credential(), now, Duration::minutes(9)).unwrap();
        let jwt = jwt.expose().to_string();

        let (signed_input, encoded_signature) = jwt.rsplit_once('.').unwrap();

        let private_key = RsaPrivateKey::from_pkcs1_pem(TEST_PRIVATE_KEY_PEM.trim()).unwrap();
        let verifying_key = VerifyingKey::<Sha256>::new(private_key.to_public_key());

        let signature_bytes = URL_SAFE_NO_PAD.decode(encoded_signature).unwrap();
        let signature = rsa::pkcs1v15::Signature::try_from(signature_bytes.as_slice()).unwrap();

        verifying_key
            .verify(signed_input.as_bytes(), &signature)
            .expect("RS256 signature should verify against the App's public key");
    }

    #[test]
    fn test_jwt_signature_rejects_a_tampered_payload() {
        let jwt = sign_app_jwt(&credential(), fixed_now(), Duration::minutes(9)).unwrap();
        let jwt = jwt.expose().to_string();

        let (signed_input, encoded_signature) = jwt.rsplit_once('.').unwrap();

        let private_key = RsaPrivateKey::from_pkcs1_pem(TEST_PRIVATE_KEY_PEM.trim()).unwrap();
        let verifying_key = VerifyingKey::<Sha256>::new(private_key.to_public_key());

        let signature_bytes = URL_SAFE_NO_PAD.decode(encoded_signature).unwrap();
        let signature = rsa::pkcs1v15::Signature::try_from(signature_bytes.as_slice()).unwrap();

        // Swap an App ID for another; the signature must no longer verify.
        let tampered = signed_input.replace('A', "B");
        assert!(
            verifying_key
                .verify(tampered.as_bytes(), &signature)
                .is_err()
        );
    }

    #[test]
    fn test_jwt_is_deterministic_for_a_fixed_instant() {
        // PKCS#1 v1.5 is deterministic, unlike PSS - so two signings of the same
        // claims are byte-identical. This is what makes the claims assertable.
        let a = sign_app_jwt(&credential(), fixed_now(), Duration::minutes(9)).unwrap();
        let b = sign_app_jwt(&credential(), fixed_now(), Duration::minutes(9)).unwrap();

        assert_eq!(a.expose(), b.expose());
    }

    #[test]
    fn test_jwt_debug_does_not_leak_the_token() {
        let jwt = sign_app_jwt(&credential(), fixed_now(), Duration::minutes(9)).unwrap();
        let rendered = format!("{:?}", jwt);

        assert!(rendered.contains("REDACTED"));
        assert!(!rendered.contains('.'));
    }
}
