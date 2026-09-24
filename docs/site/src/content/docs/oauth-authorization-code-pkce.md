# OAuth authorization code with PKCE

Use OAuth 2.0 authorization-code login when `snow-cli` must act in **user scope** instead of as a service account. This is the right setup when you want the CLI to operate as the signed-in human user and obtain a user-bound access token.

`snow-cli` implements this flow with:

- the ServiceNow authorization endpoint,
- a temporary localhost redirect listener, or, with `--sdk-oauth`, the ServiceNow SDK app's copy-and-paste code page,
- PKCE (`code_challenge_method=S256`),
- secure token storage in the OS keychain,
- automatic token refresh when a refresh token is available.

## What you need from ServiceNow

Before configuring the CLI, create an OAuth application in ServiceNow that supports the authorization-code flow.

### Create the OAuth application in ServiceNow

1. Navigate to **System OAuth > Application Registry**.
2. Click **New** and choose **Create an OAuth API endpoint for external clients**.
3. Fill in:
   - **Name**: `snow-cli` (or any descriptive name)
   - **Redirect URL**: `http://127.0.0.1:8080/oauth/callback`
   - **Active**: checked
4. Click **Submit**.
5. Open the newly created record and note:
   - **Client ID**
   - **Client Secret** (click the lock icon to reveal it)

> **PDI tip**: If you are using a Personal Developer Instance, the steps are the same. See [Testing with a PDI](/pdi-testing/) for a full walkthrough.

You will need:

- the instance URL, for example `https://dev123456.service-now.com`
- the OAuth `client_id`
- an allowed redirect URI that matches the CLI profile
- the scopes you want to request
- optionally, a `client_secret` if you are using a confidential client

## Default redirect URI

If you do not override the redirect settings, `snow-cli` uses:

```text
http://127.0.0.1:8080/oauth/callback
```

That means the redirect URI configured in ServiceNow should usually be:

```text
http://127.0.0.1:8080/oauth/callback
```

### Allowed redirect hosts

For safety, the CLI only allows loopback redirect hosts. Use one of:

- `127.0.0.1`
- `localhost`
- `::1`

Non-loopback hosts such as `0.0.0.0`, LAN IPs, or public hostnames are rejected.

## Step 1: create a profile

Create an OAuth2 profile that uses the authorization-code grant:

```bash
snow-cli profile add user-scope \
  --instance https://dev123456.service-now.com \
  --auth-method oauth2 \
  --client-id YOUR_CLIENT_ID \
  --oauth-grant-type authorization-code \
  --oauth-scope useraccount
```

### Important profile options

- `--client-id`: required for all OAuth2 flows
- `--oauth-grant-type authorization-code`: enables browser login with PKCE
- `--oauth-scope`: optional; defaults to `useraccount`
- `--oauth-redirect-host`: optional; defaults to `127.0.0.1`
- `--oauth-redirect-port`: optional; defaults to `8080`
- `--oauth-redirect-path`: optional; defaults to `/oauth/callback`

If you need a different redirect URI, change the profile and update the same value in ServiceNow:

```bash
snow-cli profile edit user-scope \
  --oauth-redirect-host localhost \
  --oauth-redirect-port 8484 \
  --oauth-redirect-path /callback
```

That profile would require this redirect URI in ServiceNow:

```text
http://localhost:8484/callback
```

## Step 2: log in

Start the browser-based login:

```bash
snow-cli auth login --profile user-scope
```

What happens next:

1. `snow-cli` starts a temporary local HTTP listener on the configured loopback redirect URI.
2. It generates a PKCE `code_verifier` and `code_challenge`.
3. It prints the authorization URL and tries to open it in your browser.
4. You sign in to ServiceNow and approve the OAuth request.
5. ServiceNow redirects back to the local callback URL.
6. `snow-cli` exchanges the authorization code for tokens and stores them securely.

On success, the CLI prints a JSON summary including the redirect URI, requested scope, and whether a refresh token was returned.

## Public PKCE client vs confidential client

### Public PKCE client

If your ServiceNow OAuth client is configured as a **public PKCE client**, you can omit the client secret entirely:

```bash
snow-cli auth login --profile user-scope
```

This is the preferred setup for local user login flows.

### Confidential client

If your ServiceNow OAuth client requires a secret, provide it during login:

```bash
printf '%s' "$SNOW_CLIENT_SECRET" | \
  snow-cli auth login --profile user-scope --client-secret-stdin
```

The secret is optional for authorization-code profiles, but if you provide it, `snow-cli` will also use it for token refresh requests.

## Step 3: verify the session

Check auth status:

```bash
snow-cli auth status --profile user-scope
```

Print the current access token:

```bash
snow-cli auth token --profile user-scope
```

Use the profile in normal commands:

```bash
snow-cli --profile user-scope table list incident --limit 10
```

## Use the ServiceNow SDK OAuth app (`--sdk-oauth`)

Instances that have the ServiceNow SDK installed already have a public OAuth application called **ServiceNow SDK**. Its redirect URL is the instance page `/sdk-oauth.do`, not a localhost address. After you approve access, that page displays an authorization code for you to paste into the CLI. `snow-cli auth login --sdk-oauth` supports this flow, so you can log in without creating an OAuth application or handling a client secret.

This is still authorization code with PKCE. It is **not** the OAuth device authorization flow (RFC 8628): the browser completes a normal authorization request, and the CLI exchanges the pasted code together with its PKCE verifier.

### Set up the profile

1. In ServiceNow, open **System OAuth > Application Registry** and find the active **ServiceNow SDK** record. Leave it unchanged.
2. Copy its **Client ID**. At the time of writing, `@servicenow/sdk-cli` uses `543e5655f77746a28228c6009a599dfb` by default.
3. Create an authorization-code profile with that client ID. No client secret is needed:

```bash
snow-cli profile add sdk \
  --instance https://dev123456.service-now.com \
  --auth-method oauth2 \
  --client-id YOUR_SDK_CLIENT_ID \
  --oauth-grant-type authorization-code
```

Redirect host, port, and path settings are ignored in this mode.

### Log in

```bash
snow-cli auth login --profile sdk --sdk-oauth
```

1. `snow-cli` generates a fresh `state` and PKCE `code_verifier` / `code_challenge`.
2. It prints the authorization URL with `redirect_uri=/sdk-oauth.do` and opens it unless you pass `--no-browser`.
3. You sign in and approve the request. ServiceNow shows an authorization code.
4. You paste the code at the hidden `Authorization code:` prompt. The code is not echoed, logged, or accepted as a command-line argument.
5. `snow-cli` exchanges the code at `/oauth_token.do` with the same `/sdk-oauth.do` redirect URI and the matching `code_verifier`. It sends no client secret, then stores the token set in the OS keychain.

Token refresh, `auth status --verify`, and `auth token` behave exactly as they do for the localhost flow.

Like `@servicenow/sdk-cli`, the login requests an empty scope. If you set `--oauth-scope` on the profile, that value is requested instead.

To supply the code from a script or another program, use `--code-stdin`. `snow-cli` reads stdin until end of file after it prints the URL:

```bash
{ read -rs CODE; printf '%s' "$CODE"; } | \
  snow-cli auth login --profile sdk --sdk-oauth --no-browser --code-stdin
```

Without `--code-stdin`, stdin must be a terminal. Empty input aborts the login before any token request.

## Headless or remote-browser workflows

If you do not want the CLI to open the browser automatically, use `--no-browser`:

```bash
snow-cli auth login --profile user-scope --no-browser
```

The CLI prints the authorization URL so you can copy it into a browser manually.

## Example `config.toml` entry

After creating the profile, the config file contains non-secret metadata similar to:

```toml
default_profile = "user-scope"

[profiles.user-scope]
instance = "https://dev123456.service-now.com"
auth_method = "oauth2"
client_id = "YOUR_CLIENT_ID"
oauth_grant_type = "authorization_code"
oauth_scope = "useraccount"
```

Redirect settings appear only when you override the defaults.

## Troubleshooting

### Redirect URI mismatch

If ServiceNow says the redirect URI is invalid, make sure these values match exactly:

- the redirect URI configured in ServiceNow
- the `snow-cli` profile redirect host, port, and path
- the actual URI printed by `snow-cli auth login`

### `--sdk-oauth` is rejected

`--sdk-oauth` works only with `auth_method = "oauth2"` and `oauth_grant_type = "authorization_code"`. For other profiles, drop the flag or change the profile with `snow-cli profile edit <name> --auth-method oauth2 --oauth-grant-type authorization-code`.

### Port already in use

If the CLI cannot bind the redirect listener, choose a different port:

```bash
snow-cli profile edit user-scope --oauth-redirect-port 8484
```

Then update the ServiceNow OAuth client redirect URI to the same port.

### Login timed out

The CLI waits up to five minutes for the OAuth redirect. Re-run `snow-cli auth login` if the browser flow was interrupted.

### Scope issues

If the token does not have the permissions you expect, review the requested OAuth scopes. `useraccount` is the default scope used by `snow-cli` when you do not specify `--oauth-scope`.

### Accidentally reusing a confidential-client secret

For authorization-code profiles, if you omit `--client-secret` on a later login, `snow-cli` clears any previously stored secret for that profile. This helps public PKCE clients avoid sending a stale secret by accident.

## Related pages

- [`Configuration and authentication`](/configuration/)
- [`profile` command reference](/commands/profile/)
- [`auth` command reference](/commands/auth/)
