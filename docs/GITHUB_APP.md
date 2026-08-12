# GitHub App Authentication

Sigilforge can hold a **GitHub App** registration and mint short-lived
installation tokens from it. This is the credential to use when a machine - CI, a
Kubernetes controller, a script - needs read access to private repositories.

## Why a GitHub App

The alternatives are worse:

| Option | Problem |
|--------|---------|
| Personal access token | Scoped to a person, not an org. Broad. Long-lived. Dies when they leave. |
| Deploy key | Per-repository, and many orgs disable them (`deploy_keys_enabled_for_repositories: false`). |
| OAuth app + user token | Needs a browser and a human, which a cluster does not have. |
| **GitHub App** | Org-owned, scoped to named repositories, no browser, tokens expire in an hour. |

## How the authentication works

A GitHub App does **not** use any of the OAuth flows in [`sigilforge-core/src/oauth/`].
There is no authorization code, no PKCE, and no refresh token. The exchange is:

```text
  app id + installation id + PEM private key   (stored once, in the OS keyring)
                    |
                    v
  RS256 JWT   iss = app id, iat = now - 60s, exp <= now + 10 minutes
                    |
                    v
  POST https://api.github.com/app/installations/{installation_id}/access_tokens
       Authorization: Bearer <jwt>
                    |
                    v
  { "token": "ghs_...", "expires_at": "...Z" }   valid ~1 hour
```

Sigilforge caches the installation token and re-mints it only when it is within
five minutes of expiry. Minting per request would waste a round trip and burn
rate limit for nothing.

[`sigilforge-core/src/oauth/`]: ../sigilforge-core/src/oauth/

## One-time setup (a human, in a browser)

This part cannot be automated - GitHub requires it. Do it once, then never again.

### 1. Create the App

Go to **Organization settings -> Developer settings -> GitHub Apps -> New GitHub App**
(`https://github.com/organizations/<ORG>/settings/apps/new`).

- **GitHub App name**: something recognisable, e.g. `<org>-argocd`
- **Homepage URL**: anything; it is required but unused
- **Webhook**: **uncheck "Active"** - nothing here listens for webhooks
- **Repository permissions**: set **Contents: Read-only**
  - Argo CD needs nothing else to sync manifests
  - Add **Metadata: Read-only** if the form does not add it for you (it usually does)
- **Where can this GitHub App be installed?**: *Only on this account*

Click **Create GitHub App**.

### 2. Note the App ID

On the App's settings page, near the top: **App ID**. A number, e.g. `1234567`.
Write it down.

### 3. Generate a private key

Same page, scroll to **Private keys** -> **Generate a private key**. The browser
downloads a `.pem` file (`<app-name>.<date>.private-key.pem`).

This file is the credential. It is shown to you once. In a moment it goes into
the OS keyring and the download should be deleted.

### 4. Install the App on the org

**Install App** in the left sidebar -> **Install** next to your org.

Choose **Only select repositories** and pick exactly the repositories that need
access - for example `raibid-fish` and `spark-infra`. "All repositories" throws
away most of the point of using an App.

### 5. Note the installation ID

After installing you land on
`https://github.com/organizations/<ORG>/settings/installations/<INSTALLATION_ID>`.

The trailing number is the **installation ID**, e.g. `89012345`. Write it down.
(If you navigate away: **Settings -> GitHub Apps -> Configure** next to the App;
the URL has the same shape.)

You now have three values: **App ID**, **installation ID**, and a **`.pem` file**.

## Register it with Sigilforge

```bash
sigilforge github-app register raibid-labs \
    --app-id 1234567 \
    --installation-id 89012345 \
    --key-file ~/Downloads/raibid-labs-argocd.2026-01-01.private-key.pem
```

The account name (`raibid-labs` here) is how you refer to this installation
later; the org name is the obvious choice.

The private key goes into the OS keyring. It is never written to a config file,
never logged, and never included in `Debug` output. **Delete the downloaded
`.pem` now** - Sigilforge has it:

```bash
shred -u ~/Downloads/raibid-labs-argocd.2026-01-01.private-key.pem
```

The key can also come from stdin, which avoids it ever touching disk:

```bash
pbpaste | sigilforge github-app register raibid-labs \
    --app-id 1234567 --installation-id 89012345
```

`register` verifies the key parses **and** reads it back out of the store before
reporting success, so a keyring that silently drops writes fails here rather than
an hour later in a cluster.

## Using it

### List registrations

```console
$ sigilforge github-app list
Registered GitHub Apps:
  github-app/raibid-labs
    App ID:          1234567
    Installation ID: 89012345
    Cached token:    expires 2026-01-01 13:05:12 UTC
    Reference:       auth://github-app/raibid-labs/installation_token
```

### Print an installation token

```bash
sigilforge github-app token raibid-labs
# ghs_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

sigilforge github-app token raibid-labs --format json
sigilforge github-app token raibid-labs --force   # re-mint even if cached
```

Use it like any bearer token:

```bash
curl -H "Authorization: Bearer $(sigilforge github-app token raibid-labs)" \
     https://api.github.com/repos/raibid-labs/raibid-fish
```

Or for git over HTTPS (`x-access-token` is the required username):

```bash
git clone "https://x-access-token:$(sigilforge github-app token raibid-labs)@github.com/raibid-labs/raibid-fish.git"
```

### Resolve via `auth://`

```bash
sigilforge resolve auth://github-app/raibid-labs/installation_token
```

The full set of addressable credentials:

| Reference | Resolves to |
|-----------|-------------|
| `auth://github-app/{account}/installation_token` | a current installation token, minted if needed |
| `auth://github-app/{account}/token` | the same (alias, for the generic `.../token` shape) |
| `auth://github-app/{account}/app_id` | the App ID |
| `auth://github-app/{account}/installation_id` | the installation ID |
| `auth://github-app/{account}/private_key` | the PEM private key |

`github-app` is a reserved service id; `{account}` names the installation.

### Remove a registration

```bash
sigilforge github-app remove raibid-labs
```

## Argo CD

Argo CD reads repository credentials from `Secret`s in its own namespace labelled
`argocd.argoproj.io/secret-type: repository`. Sigilforge renders that manifest
from a stored registration:

```bash
sigilforge github-app argocd-secret raibid-labs \
    --repo-url https://github.com/raibid-labs/raibid-fish.git
```

Output:

```yaml
# Generated by sigilforge. CONTAINS A GITHUB APP PRIVATE KEY.
# Pipe this to `kubectl apply -f -`, or encrypt it (SOPS / sealed-secrets)
# before it touches disk. Do not commit it.
apiVersion: v1
kind: Secret
metadata:
  name: repo-raibid-labs-raibid-fish
  namespace: argocd
  labels:
    argocd.argoproj.io/secret-type: repository
type: Opaque
stringData:
  type: git
  url: "https://github.com/raibid-labs/raibid-fish.git"
  githubAppID: "1234567"
  githubAppInstallationID: "89012345"
  githubAppPrivateKey: |
    -----BEGIN RSA PRIVATE KEY-----
    ...
    -----END RSA PRIVATE KEY-----
```

### This manifest contains a private key

The command writes to **stdout** and applies nothing. That is deliberate. Handle
the output one of two ways:

**Pipe it straight into the cluster** - it never touches disk:

```bash
sigilforge github-app argocd-secret raibid-labs \
    --repo-url https://github.com/raibid-labs/raibid-fish.git \
  | kubectl apply -f -
```

**Or encrypt it before storing it**, if the secret belongs in a GitOps repo:

```bash
sigilforge github-app argocd-secret raibid-labs \
    --repo-url https://github.com/raibid-labs/raibid-fish.git \
  | sops --encrypt --input-type yaml --output-type yaml /dev/stdin \
  > deploy/secrets/repo-raibid-fish.enc.yaml

# or, with sealed-secrets
sigilforge github-app argocd-secret raibid-labs \
    --repo-url https://github.com/raibid-labs/raibid-fish.git \
  | kubeseal --format yaml > deploy/secrets/repo-raibid-fish.sealed.yaml
```

**Never commit the plaintext.** A GitHub App private key in Git is worse than the
personal access token it replaced: it is org-scoped and it does not expire.

### Options

| Flag | Default | Purpose |
|------|---------|---------|
| `--repo-url` | required | Repository the credential applies to |
| `--name` | derived from the URL (`repo-<owner>-<repo>`) | Secret name |
| `--namespace` | `argocd` | Namespace Argo CD watches |
| `--project` | none | Restrict the credential to one Argo CD project |

Argo CD refreshes the installation token itself from the App ID, installation ID,
and private key in the secret - it does not call back into Sigilforge. Sigilforge
is where the key lives and where the manifest is generated from; the cluster gets
a self-sufficient copy.

Run the command once per repository. Two repositories in the same installation
share the App and key but need one `Secret` each.

## Library use

```rust,ignore
use sigilforge_core::{
    AccountId, KeyringStore,
    github_app::{GitHubAppCredential, GitHubAppTokenManager},
};

let store = KeyringStore::try_new("sigilforge")?;
let manager = GitHubAppTokenManager::new(store);
let account = AccountId::new("raibid-labs");

// Cached; mints only when the cached token is near expiry.
let token = manager.ensure_installation_token(&account).await?;
```

`GitHubAppTokenManager` also implements `TokenManager` and `ReferenceResolver`,
so it drops into generic code. To resolve GitHub App references through the
shared resolver, attach it:

```rust,ignore
use std::sync::Arc;
use sigilforge_core::resolve::DefaultReferenceResolver;

let resolver = DefaultReferenceResolver::new(store, oauth_token_manager)
    .with_github_app(Arc::new(manager));
```

Without `with_github_app`, `auth://github-app/...` returns
`ResolveError::NotConfigured` rather than silently serving a stale cached token.

For GitHub Enterprise Server, point the manager at your API root:

```rust,ignore
let manager = GitHubAppTokenManager::new(store)
    .with_api_base_url("https://ghe.example.com/api/v3");
```

## Storage keys

```text
sigilforge/github-app/{account}/app_id
sigilforge/github-app/{account}/installation_id
sigilforge/github-app/{account}/private_key          <- the PEM
sigilforge/github-app/{account}/installation_token   <- cache
sigilforge/github-app/{account}/token_expiry         <- cache, RFC 3339
```

The account is also recorded in `accounts.json` under the service `github-app`,
because platform keyrings cannot enumerate keys - that file is the only way
`github-app list` knows what exists.

## Troubleshooting

| Symptom | Cause |
|---------|-------|
| `401: the App JWT was rejected` | Wrong App ID, a key from a different App, or a local clock more than ~60s fast. Check `timedatectl`. |
| `404: installation not found` | Wrong installation ID, or the App was uninstalled from the org. |
| `403: the App lacks permission` | The App does not have Contents: Read, or the repository is outside the installation's selected set. |
| `invalid GitHub App private key` | The file is not a PEM RSA key. GitHub issues PKCS#1 (`BEGIN RSA PRIVATE KEY`); PKCS#8 (`BEGIN PRIVATE KEY`) is also accepted. |
| `the secret store accepted the private key but did not return it on read` | The OS keyring is not functional (locked, or no D-Bus session). See [TROUBLESHOOTING.md](TROUBLESHOOTING.md). |
| Argo CD reports `authentication required` | The `Secret` is in the wrong namespace, missing the `secret-type: repository` label, or its `url` does not match the URL in the `Application` spec (including the `.git` suffix). |

## Key rotation

Generate a new private key on the App's settings page, register it over the old
one, then delete the old key on GitHub:

```bash
sigilforge github-app register raibid-labs \
    --app-id 1234567 --installation-id 89012345 \
    --key-file ~/Downloads/new-key.pem
```

Re-registering discards the cached installation token, so the next call mints
with the new key. Any Argo CD `Secret`s need re-generating and re-applying.
