# OAuth 2.0 Setup (User-Scoped Login)

Use when the user asks to configure OAuth 2.0 login for `snow-cli` / `snow-cli-ro`, connect to an SSO/MFA instance without Basic auth, reuse the OOB **ServiceNow SDK** OAuth record (copy/paste code from a separate browser window), or create a custom ServiceNow OAuth Application Registry record with a localhost callback.

**Agent safety rule:** Do **not** run `profile add/edit/default` or `auth login` unless the user explicitly asks. For interactive browser login, give the user the exact command to run in their own terminal — never ask the user to paste an authorization code, password, or client secret into chat.

## 1. Choose the OAuth flow

| Flow | ServiceNow setup | Browser UX | Best for |
|---|---|---|---|
| **Option A: OOB ServiceNow SDK (`--sdk-oauth`)** | **None** — reuses the built-in active **ServiceNow SDK** `oauth_entity` (`/sdk-oauth.do`, public PKCE, no client secret) | Opens `/oauth_auth.do` (or pass `--no-browser` to open manually in a separate window); copy the displayed code from `/sdk-oauth.do` and paste into the terminal prompt | Fastest setup; no ServiceNow admin needed; works over SSH/containers/remote terminals where localhost callback cannot be reached |
| **Option B: Custom OAuth app + localhost callback** | Create one record in **System OAuth > Application Registry** with Redirect URL `http://127.0.0.1:8080/oauth/callback` | Opens browser, signs in, automatically redirects back to `snow-cli`'s temporary local listener (no copy/paste) | Zero copy/paste login; instances without the ServiceNow SDK app; custom scopes or token lifespans |

Both flows use **Authorization Code + PKCE (S256)**, support SSO/MFA, store the token set securely, and automatically refresh expired access tokens using the stored `refresh_token`.

---

## 2. Option A: OOB ServiceNow SDK OAuth (`--sdk-oauth`, manual code paste)

Instances with the ServiceNow SDK installed already ship a public OAuth client named **ServiceNow SDK** whose redirect URI is `/sdk-oauth.do`. After sign-in and approval, ServiceNow renders an authorization code on screen for you to paste into the CLI prompt.

> Note: Although pasting the code from a separate browser window feels like a device-code flow, ServiceNow implements this as standard Authorization Code + PKCE with a manual copy/paste callback (`/sdk-oauth.do`), not RFC 8628 device authorization.

### Step 1: Confirm the Client ID

1. In ServiceNow, open **System OAuth > Application Registry** and locate the active **ServiceNow SDK** record. Leave it unchanged.
2. Copy its **Client ID**. The standard `@servicenow/sdk-cli` Client ID is `543e5655f77746a28228c6009a599dfb`.
3. If you already have another working profile on that instance, you can confirm the record from the CLI (never project `client_secret`):

```bash
snow-cli-ro table list oauth_entity \
  --query 'name=ServiceNow SDK^active=true' \
  --fields sys_id,name,client_id,redirect_url,public_client,active --limit 5
```

### Step 2: Create the profile

Create an authorization-code profile with that Client ID. Omit `--oauth-scope` so `snow-cli` sends the empty scope expected by the ServiceNow SDK app (redirect host/port/path flags are ignored in `--sdk-oauth` mode):

```bash
snow-cli-ro profile add sdk \
  --instance https://<instance>.service-now.com \
  --auth-method oauth2 \
  --client-id 543e5655f77746a28228c6009a599dfb \
  --oauth-grant-type authorization-code
```

### Step 3: Log in from your terminal

Have the user run in their terminal:

```bash
# Prints the URL and opens it in your default browser:
snow-cli-ro auth login --profile sdk --sdk-oauth

# Or print the URL only so you can open it in a separate browser window / profile:
snow-cli-ro auth login --profile sdk --sdk-oauth --no-browser
```

What the user does next:
1. Open the printed authorization URL (if `--no-browser` was used, paste it into any separate browser window or SSO profile).
2. Sign in to ServiceNow and click **Allow**.
3. ServiceNow opens `/sdk-oauth.do` and displays an authorization code.
4. Copy the code and paste it at the hidden `Authorization code:` prompt in the terminal (press Enter). The code is never echoed or accepted as a CLI flag.
5. `snow-cli` exchanges the code with PKCE S256 and stores the access + refresh token.

For scripted stdin piping (instead of an interactive TTY prompt), pass `--code-stdin` with `--sdk-oauth`:

```bash
snow-cli-ro auth login --profile sdk --sdk-oauth --no-browser --code-stdin
```

### Step 4: Verify (and optionally set default)

```bash
snow-cli-ro auth status --profile sdk --verify
snow-cli-ro profile default sdk
```

---

## 3. Option B: Custom OAuth App with Localhost Callback

Use when you want automatic browser callback without copying a code, need custom scopes/token lifespans, or the **ServiceNow SDK** record is not present on the instance.

### Step 1: Create the OAuth record in ServiceNow

**Via ServiceNow UI:**
1. Navigate to **System OAuth > Application Registry** (`oauth_entity`).
2. Click **New** → **Create an OAuth API endpoint for external clients**.
3. Configure:
   - **Name**: `snow-cli` (or another descriptive name)
   - **Redirect URL**: `http://127.0.0.1:8080/oauth/callback` (default; must match the CLI profile byte-for-byte)
   - **Inbound Grant Type** (on newer releases): **Authorization Code** (`authz_code`)
   - **Public Client**: checked (recommended — PKCE without storing a client secret; add the field via Form Layout if hidden on the classic view). Leave unchecked only if policy requires a confidential client secret.
   - **Active**: checked
   - Optional: adjust **Access Token Lifespan** and **Refresh Token Lifespan** per org policy.
4. Click **Submit**, reopen the record, and copy **Client ID** (and **Client Secret** only if **Public Client** is unchecked).

**Or via full `snow-cli` (only when the user asks and already has an admin profile):**

```bash
snow-cli table create oauth_entity \
  --data '{"name":"snow-cli","redirect_url":"http://127.0.0.1:8080/oauth/callback","public_client":true,"inbound_grant_type":"authz_code","active":true}'
```

Copy `client_id` from the JSON response.

### Step 2: Create the profile in `snow-cli`

Default callback (`http://127.0.0.1:8080/oauth/callback`, scope `useraccount`):

```bash
snow-cli-ro profile add dev-oauth \
  --instance https://<instance>.service-now.com \
  --auth-method oauth2 \
  --client-id <client_id> \
  --oauth-grant-type authorization-code \
  --oauth-scope useraccount
```

If port `8080` is busy or you registered a different loopback Redirect URL in ServiceNow (allowed hosts: `127.0.0.1`, `localhost`, `::1` only), configure matching callback settings on the profile:

```bash
snow-cli-ro profile edit dev-oauth \
  --oauth-redirect-host localhost \
  --oauth-redirect-port 8484 \
  --oauth-redirect-path /callback
```

That profile expects the ServiceNow **Redirect URL** to be `http://localhost:8484/callback`.

### Step 3: Log in from your terminal

Have the user run in their terminal:

```bash
# Public PKCE client (recommended — no client secret needed):
snow-cli-ro auth login --profile dev-oauth

# Print the URL instead of auto-opening the browser:
snow-cli-ro auth login --profile dev-oauth --no-browser
```

If the ServiceNow record is a **confidential client** (**Public Client** unchecked), pipe the secret on stdin (never pass secrets on the command line or in chat):

```bash
printf '%s' "$SNOW_CLI_CLIENT_SECRET" | \
  snow-cli-ro auth login --profile dev-oauth --client-secret-stdin
```

What happens: `snow-cli` binds the temporary loopback listener, opens/prints the authorization URL with PKCE S256 and `state`, receives the redirect automatically after approval (5-minute timeout), exchanges the code at `/oauth_token.do`, and stores the token set.

### Step 4: Verify (and optionally set default)

```bash
snow-cli-ro auth status --profile dev-oauth --verify
snow-cli-ro profile default dev-oauth
```

---

## 4. Troubleshooting & gotchas

- **Redirect URI mismatch:** The ServiceNow **Redirect URL**, the profile's `--oauth-redirect-host` / `--oauth-redirect-port` / `--oauth-redirect-path`, and the `redirect_uri` printed by `auth login` must match exactly (including scheme `http://` and path).
- **Port already in use:** Change the port with `snow-cli-ro profile edit dev-oauth --oauth-redirect-port 8484` and update the ServiceNow **Redirect URL** to match.
- **`--sdk-oauth` rejected:** Requires `auth_method = "oauth2"` and `oauth_grant_type = "authorization_code"`. Fix with `snow-cli-ro profile edit sdk --auth-method oauth2 --oauth-grant-type authorization-code`.
- **Non-TTY error with `--sdk-oauth`:** Running `--sdk-oauth` from a non-interactive shell fails unless `--code-stdin` is passed; run `auth login` directly in an interactive terminal.
- **Switching from confidential to public PKCE:** Omitting `--client-secret` / `--client-secret-stdin` on a subsequent authorization-code login automatically deletes any stale stored `client_secret` for that profile.
- **Inspecting tokens:** `snow-cli auth token --profile dev-oauth` (full binary only) prints the current short-lived access token for piping to other tools; `snow-cli-ro` intentionally omits `auth token`.
