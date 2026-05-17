# Testing with a Personal Developer Instance (PDI)

A ServiceNow [Personal Developer Instance](https://developer.servicenow.com/dev.do) (PDI) is a free, full-featured instance you can use to test `snow-cli` safely without affecting production data.

## Request a PDI

1. Go to [https://developer.servicenow.com/dev.do](https://developer.servicenow.com/dev.do) and sign in.
2. Click **Request Instance** and choose a release family.
3. Wait for the instance to be provisioned. You will receive:
   - Instance URL, e.g. `https://dev123456.service-now.com`
   - Username, usually `admin`
   - Password (displayed once; save it securely)

## Quick test with basic auth

The fastest way to verify `snow-cli` against your PDI is basic authentication.

### 1. Create a profile

```bash
snow-cli profile add pdi \
  --instance https://dev123456.service-now.com \
  --auth-method basic \
  --username admin
```

Replace `https://dev123456.service-now.com` with your actual instance URL.

### 2. Log in

```bash
snow-cli auth login --profile pdi
```

When prompted, enter the admin password from your PDI request.

### 3. Verify

```bash
snow-cli auth status --profile pdi
snow-cli --profile pdi table list incident --limit 5
```

## Testing read-only mode

`snow-cli-ro` is a read-only variant of the CLI. It is useful when you want to guarantee that no mutations happen on your PDI.

```bash
snow-cli-ro --profile pdi table list incident --limit 5
```

`snow-cli-ro` behaves exactly like `snow-cli --read-only`. Any command that would write, update, or delete data is blocked before it reaches the network.

## Setting up OAuth on a PDI

If you want to test OAuth2 flows, you can create an OAuth application inside your PDI.

### 1. Create an OAuth application

1. In your PDI, navigate to **System OAuth > Application Registry**.
2. Click **New** and choose **Create an OAuth API endpoint for external clients**.
3. Fill in:
   - **Name**: `snow-cli`
   - **Redirect URL**: `http://127.0.0.1:8080/oauth/callback`
   - **Active**: checked
4. Click **Submit**.
5. Open the newly created record and note:
   - **Client ID**
   - **Client Secret** (click the lock icon to reveal it)

### 2. Create an authorization-code profile

```bash
snow-cli profile add pdi-oauth \
  --instance https://dev123456.service-now.com \
  --auth-method oauth2 \
  --client-id YOUR_CLIENT_ID \
  --oauth-grant-type authorization-code
```

### 3. Log in

```bash
snow-cli auth login --profile pdi-oauth
```

The CLI opens a browser window. Sign in to your PDI and approve the OAuth request.

### 4. Verify

```bash
snow-cli --profile pdi-oauth table list incident --limit 5
```

## PDI-specific tips

- **Wakeup**: PDIs hibernate after a period of inactivity. The first request after wakeup may take 30–60 seconds. Increase the timeout if needed:
  ```bash
  snow-cli --profile pdi --timeout-secs 120 table list incident --limit 5
  ```
- **Instance release**: PDIs are recreated on major release upgrades. Save your profile config and re-create the profile on a new instance if needed.
- **Data safety**: PDIs are meant for experimentation. Feel free to create, update, and delete records. If something breaks, request a new instance.

## Related pages

- [Quick start](./quick-start.md)
- [OAuth authorization code with PKCE](./oauth-authorization-code-pkce.md)
- [Configuration and authentication](./configuration.md)
