//! Integration tests for GitHub App installation-token minting.
//!
//! The GitHub API is mocked with `wiremock`; nothing here touches the network.
//! These tests cover the parts that unit tests in `github_app::token` cannot:
//! the shape of the outbound request, the caching decision across calls, and
//! how API failures surface.

#![cfg(feature = "github-app")]

use std::sync::Arc;

use chrono::{Duration, Utc};
use sigilforge_core::{
    AccountId, ServiceId,
    github_app::{
        GitHubAppCredential, GitHubAppError, GitHubAppTokenManager, github_app_service_id,
        test_support::TEST_PRIVATE_KEY_PEM,
    },
    resolve::{ReferenceResolver, ResolveError},
    store::{MemoryStore, Secret, SecretStore},
    token::{Token, TokenManager},
};
use wiremock::matchers::{header, header_exists, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

const APP_ID: u64 = 1234567;
const INSTALLATION_ID: u64 = 89012345;
const ACCESS_TOKENS_PATH: &str = "/app/installations/89012345/access_tokens";

fn account() -> AccountId {
    AccountId::new("raibid-labs")
}

fn credential() -> GitHubAppCredential {
    GitHubAppCredential::new(APP_ID, INSTALLATION_ID, TEST_PRIVATE_KEY_PEM).unwrap()
}

/// A manager pointed at a mock GitHub, with a registration already stored.
async fn registered_manager(server: &MockServer) -> GitHubAppTokenManager<MemoryStore> {
    let manager = GitHubAppTokenManager::new(MemoryStore::new()).with_api_base_url(server.uri());
    manager.register(&account(), &credential()).await.unwrap();
    manager
}

/// A successful `access_tokens` response expiring `minutes` from now.
fn minted_token(token: &str, minutes: i64) -> ResponseTemplate {
    ResponseTemplate::new(201).set_body_json(serde_json::json!({
        "token": token,
        "expires_at": (Utc::now() + Duration::minutes(minutes)).to_rfc3339(),
        "permissions": { "contents": "read" },
        "repository_selection": "selected",
    }))
}

#[tokio::test]
async fn mints_a_token_and_returns_it() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_minted", 60))
        .expect(1)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let token = manager.ensure_installation_token(&account()).await.unwrap();

    assert_eq!(token.access_token.expose(), "ghs_minted");
    assert!(token.expires_at.is_some());
}

#[tokio::test]
async fn sends_a_bearer_jwt_and_the_required_github_headers() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .and(header("accept", "application/vnd.github+json"))
        .and(header("x-github-api-version", "2022-11-28"))
        .and(header_exists("user-agent"))
        .respond_with(minted_token("ghs_minted", 60))
        .expect(1)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    manager.ensure_installation_token(&account()).await.unwrap();

    // The Authorization header must carry a three-segment RS256 JWT, not the
    // installation token and not a raw PEM.
    let requests = server.received_requests().await.unwrap();
    let authorization = authorization_header(&requests[0]);
    let assertion = authorization
        .strip_prefix("Bearer ")
        .expect("bearer scheme");

    assert_eq!(assertion.split('.').count(), 3);
    assert!(!assertion.contains("PRIVATE KEY"));
}

#[tokio::test]
async fn reuses_a_cached_token_instead_of_minting_again() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_minted", 60))
        .expect(1) // the point of the test: exactly one mint for two calls
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;

    let first = manager.ensure_installation_token(&account()).await.unwrap();
    let second = manager.ensure_installation_token(&account()).await.unwrap();

    assert_eq!(first.access_token.expose(), second.access_token.expose());
}

#[tokio::test]
async fn re_mints_when_the_cached_token_is_inside_the_refresh_buffer() {
    let server = MockServer::start().await;

    // Expires in two minutes, inside the five-minute default buffer.
    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_nearly_expired", 2))
        .expect(2)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;

    manager.ensure_installation_token(&account()).await.unwrap();
    manager.ensure_installation_token(&account()).await.unwrap();
}

#[tokio::test]
async fn re_mints_when_the_cached_token_has_already_expired() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_fresh", 60))
        .expect(1)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;

    // Seed the cache with a token that expired an hour ago.
    manager
        .store_token_set(
            &github_app_service_id(),
            &account(),
            sigilforge_core::token::TokenSet::new(
                Token::new("ghs_stale").with_expiry(Utc::now() - Duration::hours(1)),
            ),
        )
        .await
        .unwrap();

    let token = manager.ensure_installation_token(&account()).await.unwrap();
    assert_eq!(token.access_token.expose(), "ghs_fresh");
}

#[tokio::test]
async fn mint_bypasses_the_cache_entirely() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_forced", 60))
        .expect(2)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;

    manager.ensure_installation_token(&account()).await.unwrap();
    manager.mint_installation_token(&account()).await.unwrap();
}

#[tokio::test]
async fn caches_the_minted_token_in_the_secret_store() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_minted", 60))
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    manager.ensure_installation_token(&account()).await.unwrap();

    let cached = manager
        .store()
        .get("sigilforge/github-app/raibid-labs/installation_token")
        .await
        .unwrap()
        .unwrap();

    assert_eq!(cached.expose(), "ghs_minted");
    assert!(
        manager
            .store()
            .exists("sigilforge/github-app/raibid-labs/token_expiry")
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn surfaces_a_401_with_an_actionable_message() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({ "message": "Bad credentials" })),
        )
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let err = manager
        .ensure_installation_token(&account())
        .await
        .unwrap_err();

    match err {
        GitHubAppError::GitHubApi { status, message } => {
            assert_eq!(status, 401);
            assert!(message.contains("App ID"), "unhelpful message: {}", message);
        }
        other => panic!("expected GitHubApi error, got {:?}", other),
    }
}

#[tokio::test]
async fn surfaces_a_404_as_an_installation_problem() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let err = manager
        .ensure_installation_token(&account())
        .await
        .unwrap_err();

    assert!(err.to_string().contains("installation ID"));
}

#[tokio::test]
async fn rejects_a_malformed_success_response() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(serde_json::json!({ "token": "ghs_x" })),
        )
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let err = manager
        .ensure_installation_token(&account())
        .await
        .unwrap_err();

    assert!(matches!(err, GitHubAppError::InvalidResponse { .. }));
}

#[tokio::test]
async fn a_failed_mint_leaves_no_token_cached() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    assert!(manager.ensure_installation_token(&account()).await.is_err());
    assert!(manager.cached_token(&account()).await.unwrap().is_none());
}

#[tokio::test]
async fn unregistered_accounts_never_reach_the_network() {
    let server = MockServer::start().await;

    // No mock is mounted: any outbound request would 404 the test, not the App.
    let manager = GitHubAppTokenManager::new(MemoryStore::new()).with_api_base_url(server.uri());

    let err = manager
        .ensure_installation_token(&AccountId::new("never-registered"))
        .await
        .unwrap_err();

    assert!(matches!(err, GitHubAppError::NotRegistered { .. }));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn auth_uri_resolves_to_a_fresh_installation_token() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_resolved", 60))
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let value = manager
        .resolve("auth://github-app/raibid-labs/installation_token")
        .await
        .unwrap();

    assert!(value.is_secret());
    assert_eq!(value.expose(), "ghs_resolved");
}

#[tokio::test]
async fn auth_uri_accepts_token_as_an_alias() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_resolved", 60))
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let value = manager
        .resolve("auth://github-app/raibid-labs/token")
        .await
        .unwrap();

    assert_eq!(value.expose(), "ghs_resolved");
}

#[tokio::test]
async fn auth_uri_resolves_the_private_key_from_the_store() {
    let server = MockServer::start().await;
    let manager = registered_manager(&server).await;

    let value = manager
        .resolve("auth://github-app/raibid-labs/private_key")
        .await
        .unwrap();

    assert!(value.expose().contains("BEGIN RSA PRIVATE KEY"));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn default_resolver_routes_github_app_references_to_the_app_manager() {
    use sigilforge_core::{
        provider::ProviderRegistry, resolve::DefaultReferenceResolver,
        token_manager::DefaultTokenManager,
    };

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_via_default_resolver", 60))
        .mount(&server)
        .await;

    let apps = Arc::new(registered_manager(&server).await);
    let oauth_manager = DefaultTokenManager::new(MemoryStore::new(), ProviderRegistry::new());

    let resolver = DefaultReferenceResolver::new(MemoryStore::new(), oauth_manager)
        .with_github_app(apps.clone());

    let value = resolver
        .resolve("auth://github-app/raibid-labs/installation_token")
        .await
        .unwrap();

    assert_eq!(value.expose(), "ghs_via_default_resolver");
}

#[tokio::test]
async fn default_resolver_without_an_app_manager_says_so() {
    use sigilforge_core::{
        provider::ProviderRegistry, resolve::DefaultReferenceResolver,
        token_manager::DefaultTokenManager,
    };

    let oauth_manager = DefaultTokenManager::new(MemoryStore::new(), ProviderRegistry::new());
    let resolver = DefaultReferenceResolver::new(MemoryStore::new(), oauth_manager);

    let err = resolver
        .resolve("auth://github-app/raibid-labs/installation_token")
        .await
        .unwrap_err();

    match err {
        ResolveError::NotConfigured { reference, message } => {
            assert_eq!(
                reference,
                "auth://github-app/raibid-labs/installation_token"
            );
            assert!(message.contains("with_github_app"));
        }
        other => panic!("expected NotConfigured, got {:?}", other),
    }
}

#[tokio::test]
async fn token_manager_trait_delegates_to_installation_minting() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_via_trait", 60))
        .expect(1)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let token = manager
        .ensure_access_token(&github_app_service_id(), &account())
        .await
        .unwrap();

    assert_eq!(token.access_token.expose(), "ghs_via_trait");
}

#[tokio::test]
async fn re_registering_forces_the_next_call_to_mint() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_minted", 60))
        .expect(2)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    manager.ensure_installation_token(&account()).await.unwrap();

    // A new registration invalidates a token minted under the old one.
    manager.register(&account(), &credential()).await.unwrap();
    manager.ensure_installation_token(&account()).await.unwrap();
}

#[tokio::test]
async fn a_corrupt_cached_expiry_forces_a_re_mint_rather_than_failing() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path(ACCESS_TOKENS_PATH))
        .respond_with(minted_token("ghs_fresh", 60))
        .expect(1)
        .mount(&server)
        .await;

    let manager = registered_manager(&server).await;
    let store = manager.store();

    store
        .set(
            "sigilforge/github-app/raibid-labs/installation_token",
            &Secret::new("ghs_unknown_expiry"),
        )
        .await
        .unwrap();
    store
        .set(
            "sigilforge/github-app/raibid-labs/token_expiry",
            &Secret::new("whenever"),
        )
        .await
        .unwrap();

    // An unparseable expiry means "unknown", and an unknown expiry is treated as
    // never-expiring by `Token::expires_within` - so the cached token would be
    // reused forever. Assert the safer behaviour instead: re-mint.
    let token = manager.ensure_installation_token(&account()).await.unwrap();
    assert_eq!(token.access_token.expose(), "ghs_fresh");
}

#[tokio::test]
async fn other_services_are_rejected_without_a_request() {
    let server = MockServer::start().await;
    let manager = registered_manager(&server).await;

    assert!(
        manager
            .ensure_access_token(&ServiceId::new("spotify"), &account())
            .await
            .is_err()
    );
    assert!(server.received_requests().await.unwrap().is_empty());
}

/// Read the `Authorization` header off a captured request.
fn authorization_header(request: &Request) -> String {
    request
        .headers
        .get("authorization")
        .expect("Authorization header")
        .to_str()
        .unwrap()
        .to_string()
}
