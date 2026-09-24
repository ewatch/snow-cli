# HTTP Client and Pagination Design

## Overview

The `SnowClient` wraps `reqwest::Client` and provides a high-level interface for
making authenticated requests to ServiceNow APIs. It handles pagination, error
mapping, and output formatting.

## Client Architecture

```
┌──────────────────────────────────────────────┐
│                  SnowClient                  │
│                                              │
│  ┌─────────────┐  ┌──────────────────────┐   │
│  │ reqwest      │  │ Authenticator (trait) │   │
│  │ ::Client     │  │                      │   │
│  └─────────────┘  └──────────────────────┘   │
│                                              │
│  ┌─────────────┐  ┌──────────────────────┐   │
│  │ Paginator    │  │ ErrorMapper          │   │
│  └─────────────┘  └──────────────────────┘   │
└──────────────────────────────────────────────┘
```

## Request Flow

1. Command handler calls `client.get("/api/now/table/incident", params)`.
2. `SnowClient` calls `authenticator.authenticate()` to get auth headers.
3. Request is sent with auth headers, instance base URL, and parameters.
4. Response is checked:
   - 2xx: Parse JSON body, return data.
   - 401: Attempt `authenticator.refresh()`, retry once.
   - 4xx/5xx: Map to structured `ApiError` and write JSON to stderr.

## Auto-Pagination

ServiceNow Table API returns paginated results with these response headers:

| Header                     | Description                          |
|----------------------------|--------------------------------------|
| `X-Total-Count`            | Total number of matching records     |
| `Link` (rel="next")       | URL for the next page                |

### Pagination Strategy

```rust
pub struct Paginator {
    limit: Option<usize>,    // User-specified max records (--limit)
    page_size: usize,        // Records per request (default: 100)
}

impl Paginator {
    /// Returns an async stream of records, automatically fetching next pages.
    pub fn paginate<T>(&self, client: &SnowClient, url: &str, params: &Params)
        -> impl Stream<Item = Result<T, ApiError>>
    where
        T: DeserializeOwned;
}
```

Behavior:
- Default: Fetch all records, streaming pages as they arrive.
- With `--limit N`: Stop after N total records.
- Page size is configurable but defaults to 100 (ServiceNow's common default).

## Error Mapping

ServiceNow API errors are mapped to the standard error format:

```rust
pub struct ApiError {
    pub code: String,        // e.g., "INVALID_TABLE"
    pub message: String,     // ServiceNow error.message, or a generic summary
    pub status: u16,         // HTTP status code
    pub detail: Option<String>, // ServiceNow error.detail, or a redaction note
    pub instance: String,    // Instance URL for context
}
```

### HTTP Status to Error Code Mapping

| HTTP Status | Error Code              | Meaning                          |
|-------------|-------------------------|----------------------------------|
| 400         | `BAD_REQUEST`           | Invalid query or parameters      |
| 401         | `UNAUTHORIZED`          | Auth failed (after refresh retry)|
| 403         | `FORBIDDEN`             | Insufficient permissions (ACL)   |
| 404         | `NOT_FOUND`             | Table or record not found        |
| 429         | `RATE_LIMITED`           | Too many requests                |
| 500+        | `SERVER_ERROR`          | ServiceNow internal error        |
| timeout     | `REQUEST_TIMEOUT`       | Request timed out                |
| conn error  | `CONNECTION_ERROR`      | Could not connect to instance    |

When the response body is the standard ServiceNow envelope
(`{"error":{"message":"...","detail":"..."},"status":"failure"}`), `message` and
`detail` come from the envelope after secret scrubbing (credential-like
`key=value` pairs, `Authorization`/`Bearer` values, control characters) and
bounding to 500 characters each. An unambiguous message refines the code:

| Condition                                   | Error Code         |
|---------------------------------------------|--------------------|
| message starts with `Invalid table`         | `INVALID_TABLE`    |
| 404 with `No Record found`                  | `RECORD_NOT_FOUND` |
| 403 mentioning `ACL` in message or detail   | `ACL_DENIED`       |

Any other body (HTML, unknown JSON) is never echoed: `detail` reports only its
size unless `SNOW_CLI_DEBUG_HTTP_INCLUDE_SENSITIVE` is set. Failed requests are
logged at `debug` level without the body; `SNOW_CLI_DEBUG_HTTP` is the opt-in
way to inspect raw responses.
