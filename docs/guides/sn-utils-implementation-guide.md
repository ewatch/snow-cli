# Implementation Guide: SN-Utils WebSocket Client for snow-cli

## Overview

This guide walks through implementing the SN-Utils WebSocket integration for snow-cli **step by step**. The integration allows snow-cli to use the browser's authenticated session instead of managing credentials.

---

## Phase 1: WebSocket Client Implementation

### Step 1: Add Dependencies to `Cargo.toml`

```toml
[dependencies]
# ... existing deps ...
tokio-tungstenite = "0.21"
futures = "0.3"
```

### Step 2: Create `src/auth/sn_utils_websocket.rs`

This module handles all WebSocket communication with SN-Utils.

```rust
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures::{SinkExt, StreamExt};

/// Response from SN-Utils for any action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnUtilsResponse {
    #[serde(rename = "type")]
    pub action_type: String,
    pub result: Option<Value>,
    pub error: Option<String>,
    pub instance: Option<SnUtilsInstance>,
}

/// Instance metadata passed through WebSocket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnUtilsInstance {
    pub name: String,
    pub url: String,
    pub g_ck: String,
}

/// Main WebSocket client
pub struct SnUtilsWebSocketClient {
    ws_url: String,
    instance: SnUtilsInstance,
    request_timeout: std::time::Duration,
    // Store pending responses by request ID
    pending: Arc<RwLock<HashMap<String, tokio::sync::oneshot::Sender<SnUtilsResponse>>>>,
}

impl SnUtilsWebSocketClient {
    /// Create a new WebSocket client
    pub fn new(
        instance_url: &str,
        instance_name: &str,
        g_ck: &str,
        ws_url: &str,
    ) -> Self {
        Self {
            ws_url: ws_url.to_string(),
            instance: SnUtilsInstance {
                name: instance_name.to_string(),
                url: instance_url.to_string(),
                g_ck: g_ck.to_string(),
            },
            request_timeout: std::time::Duration::from_secs(30),
            pending: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Connect and start listening for messages
    pub async fn connect(&self) -> anyhow::Result<SnUtilsWebSocketHandle> {
        // Try to connect to WebSocket
        let (ws_stream, _) = match connect_async(&self.ws_url).await {
            Ok(stream) => stream,
            Err(e) => {
                anyhow::bail!(
                    "Failed to connect to SN-Utils WebSocket at {}: {}",
                    self.ws_url,
                    e
                )
            }
        };

        // Spawn background task to handle incoming messages
        let pending = self.pending.clone();
        tokio::spawn(async move {
            let (mut write, mut read) = ws_stream.split();
            
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        // Parse response
                        if let Ok(response) = serde_json::from_str::<SnUtilsResponse>(&text) {
                            // Find matching request and resolve
                            // For now, we'll implement request correlation below
                        }
                    }
                    Err(e) => {
                        eprintln!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(SnUtilsWebSocketHandle {
            client: self.clone(),
        })
    }

    /// Send a request and wait for response
    pub async fn send_request(
        &self,
        action: &str,
        payload: Value,
    ) -> anyhow::Result<SnUtilsResponse> {
        // Build request with instance info
        let mut request = payload;
        request["action"] = Value::String(action.to_string());
        request["instance"] = serde_json::to_value(&self.instance)?;

        // Send via WebSocket
        let msg_text = serde_json::to_string(&request)?;
        
        // In real implementation:
        // 1. Generate unique request ID
        // 2. Create oneshot channel
        // 3. Store in pending map
        // 4. Send message
        // 5. Wait for response with timeout
        
        anyhow::bail!("Implementation incomplete")
    }

    /// Get a single record
    pub async fn get_record(&self, table: &str, sys_id: &str) -> anyhow::Result<Record> {
        let payload = json!({
            "tableName": table,
            "sys_id": sys_id,
        });

        let response = self.send_request("requestRecord", payload).await?;

        if let Some(result) = response.result {
            if let Some(records) = result.get("result").and_then(|r| r.as_array()) {
                if let Some(record) = records.first() {
                    return Ok(serde_json::from_value(record.clone())?);
                }
            }
        }

        anyhow::bail!("No record found")
    }

    /// Query records with filters
    pub async fn query_records(
        &self,
        table: &str,
        query: Option<&str>,
    ) -> anyhow::Result<Vec<Record>> {
        let payload = json!({
            "tableName": table,
            "query": query,
        });

        let response = self.send_request("requestRecords", payload).await?;

        if let Some(result) = response.result {
            if let Some(records) = result.get("result").and_then(|r| r.as_array()) {
                return Ok(serde_json::from_value(records.clone())?);
            }
        }

        Ok(vec![])
    }

    /// Create a new record
    pub async fn create_record(
        &self,
        table: &str,
        fields: &Value,
    ) -> anyhow::Result<Record> {
        let payload = json!({
            "tableName": table,
            "fields": fields,
        });

        let response = self.send_request("createRecord", payload).await?;

        if let Some(result) = response.result {
            return Ok(serde_json::from_value(result)?);
        }

        anyhow::bail!("Failed to create record")
    }

    /// Update an existing record
    pub async fn update_record(
        &self,
        table: &str,
        sys_id: &str,
        fields: &Value,
    ) -> anyhow::Result<Record> {
        let payload = json!({
            "tableName": table,
            "sys_id": sys_id,
            "fields": fields,
        });

        let response = self.send_request("updateRecord", payload).await?;

        if let Some(result) = response.result {
            return Ok(serde_json::from_value(result)?);
        }

        anyhow::bail!("Failed to update record")
    }

    /// Delete a record
    pub async fn delete_record(&self, table: &str, sys_id: &str) -> anyhow::Result<()> {
        let payload = json!({
            "tableName": table,
            "sys_id": sys_id,
        });

        let response = self.send_request("deleteRecord", payload).await?;

        if response.error.is_none() {
            Ok(())
        } else {
            anyhow::bail!("Failed to delete record: {:?}", response.error)
        }
    }

    /// Get table schema/metadata
    pub async fn get_table_structure(&self, table: &str) -> anyhow::Result<TableSchema> {
        let payload = json!({
            "tableName": table,
        });

        let response = self.send_request("requestTableStructure", payload).await?;

        if let Some(result) = response.result {
            return Ok(serde_json::from_value(result)?);
        }

        anyhow::bail!("Failed to get table structure")
    }
}

// Type stubs (define these properly in models/)
pub type Record = serde_json::Value;
pub type TableSchema = serde_json::Value;

/// Handle to a connected WebSocket client
pub struct SnUtilsWebSocketHandle {
    client: SnUtilsWebSocketClient,
}
```

### Step 3: Create `src/config/sn_utils_discovery.rs`

Handle g_ck token discovery from browser.

```rust
use anyhow::Result;
use std::path::Path;

/// Discover g_ck token from running browser session
pub async fn discover_g_ck_token(instance_url: &str) -> Result<String> {
    // Strategy 1: Check if SN-Utils helper tab has exported g_ck
    // This could be via:
    // - Environment variable: SN_UTILS_G_CK
    // - File in temp directory
    // - Query helper tab endpoint (if available)
    
    // For MVP, guide user to provide g_ck
    eprintln!("To use SN-Utils WebSocket, you need to:");
    eprintln!("1. Open your ServiceNow instance: {}", instance_url);
    eprintln!("2. Open SN-Utils helper tab");
    eprintln!("3. Run: snow-cli auth token --source sn-utils");
    
    anyhow::bail!("g_ck token not discovered. See instructions above.")
}

/// Verify WebSocket is reachable
pub async fn verify_websocket_ready() -> Result<()> {
    let url = "ws://127.0.0.1:1978";
    
    match tokio_tungstenite::connect_async(url).await {
        Ok(_) => Ok(()),
        Err(e) => {
            anyhow::bail!(
                "Cannot reach SN-Utils WebSocket at {}. \
                 Ensure SN-Utils helper tab is open in your browser. Error: {}",
                url,
                e
            )
        }
    }
}
```

### Step 4: Update `src/auth/mod.rs`

Add SN-Utils as an authentication method.

```rust
// In src/auth/mod.rs

pub mod basic;
pub mod oauth2;
pub mod api_key;
pub mod browser_session;
pub mod sn_utils_websocket;  // <-- NEW

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "auth_method")]
pub enum AuthMethod {
    Basic { username: String, password: String },
    OAuth2 { /* ... */ },
    ApiKey { token: String },
    BrowserSession { /* ... */ },
    SnUtils {
        instance_url: String,
        g_ck: String,
    },
}

#[async_trait]
pub trait Authenticator: Send + Sync {
    async fn authorize(&self, req: &mut reqwest::Request) -> anyhow::Result<()>;
}

// Implement Authenticator for each variant
#[async_trait]
impl Authenticator for AuthMethod {
    async fn authorize(&self, req: &mut reqwest::Request) -> anyhow::Result<()> {
        match self {
            Self::Basic { username, password } => {
                // ... existing code ...
            }
            Self::SnUtils { .. } => {
                // For SN-Utils, don't modify HTTP request directly.
                // Instead, the HTTP client will route through WebSocket.
                Ok(())
            }
            _ => { /* ... */ }
        }
    }
}
```

---

## Phase 2: Request Translation Layer

### Step 5: Create `src/client/websocket_translator.rs`

Translate REST API calls to SN-Utils actions.

```rust
use serde_json::{json, Value};
use url::Url;

/// Convert REST request to SN-Utils action
pub fn rest_to_sn_utils_action(
    method: &str,
    path: &str,
    body: Option<&str>,
) -> anyhow::Result<(String, Value)> {
    match (method, path) {
        // GET /api/now/table/incident -> requestRecords
        ("GET", p) if p.starts_with("/api/now/table/") => {
            let parts: Vec<&str> = p.split('/').collect();
            
            if parts.len() == 5 && parts[4].is_empty() {
                // /api/now/table/incident (list)
                let table = parts[4];
                let payload = json!({
                    "tableName": table,
                });
                Ok(("requestRecords".to_string(), payload))
            } else if parts.len() == 6 {
                // /api/now/table/incident/INC123 (get)
                let table = parts[4];
                let sys_id = parts[5];
                let payload = json!({
                    "tableName": table,
                    "sys_id": sys_id,
                });
                Ok(("requestRecord".to_string(), payload))
            } else {
                anyhow::bail!("Unsupported path: {}", path)
            }
        }
        // POST /api/now/table/incident -> createRecord
        ("POST", p) if p.starts_with("/api/now/table/") => {
            let parts: Vec<&str> = p.split('/').collect();
            let table = parts[4];
            let fields = parse_request_body(body)?;
            
            Ok((
                "createRecord".to_string(),
                json!({
                    "tableName": table,
                    "fields": fields,
                }),
            ))
        }
        // PATCH /api/now/table/incident/INC123 -> updateRecord
        ("PATCH", p) | ("PUT", p) if p.starts_with("/api/now/table/") => {
            let parts: Vec<&str> = p.split('/').collect();
            let table = parts[4];
            let sys_id = parts[5];
            let fields = parse_request_body(body)?;
            
            Ok((
                "updateRecord".to_string(),
                json!({
                    "tableName": table,
                    "sys_id": sys_id,
                    "fields": fields,
                }),
            ))
        }
        // DELETE /api/now/table/incident/INC123 -> deleteRecord
        ("DELETE", p) if p.starts_with("/api/now/table/") => {
            let parts: Vec<&str> = p.split('/').collect();
            let table = parts[4];
            let sys_id = parts[5];
            
            Ok((
                "deleteRecord".to_string(),
                json!({
                    "tableName": table,
                    "sys_id": sys_id,
                }),
            ))
        }
        _ => anyhow::bail!("Unsupported: {} {}", method, path),
    }
}

fn parse_request_body(body: Option<&str>) -> anyhow::Result<Value> {
    match body {
        Some(b) => Ok(serde_json::from_str(b)?),
        None => Ok(json!({})),
    }
}
```

---

## Phase 3: Integration with HTTP Client

### Step 6: Update `src/client/mod.rs`

Route requests through WebSocket for SN-Utils auth.

```rust
// In src/client/mod.rs

impl SnowClient {
    pub async fn execute(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> anyhow::Result<String> {
        match &self.auth_method {
            AuthMethod::SnUtils { instance_url, g_ck } => {
                self.execute_via_websocket(method, path, body).await
            }
            _ => self.execute_http_direct(method, path, body).await,
        }
    }

    async fn execute_via_websocket(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> anyhow::Result<String> {
        // Translate REST to SN-Utils action
        let (action, payload) = websocket_translator::rest_to_sn_utils_action(
            method, path, body,
        )?;

        // Connect to WebSocket
        let instance_url = self.instance_url.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Instance URL required for SN-Utils")
        })?;

        let client = sn_utils_websocket::SnUtilsWebSocketClient::new(
            instance_url,
            "snow-cli",
            g_ck,
            "ws://127.0.0.1:1978",
        );

        // Send request
        let response = client.send_request(&action, payload).await?;

        // Convert response back to JSON
        if let Some(result) = response.result {
            Ok(serde_json::to_string(&result)?)
        } else {
            Err(anyhow::anyhow!("No result: {:?}", response.error))
        }
    }

    async fn execute_http_direct(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> anyhow::Result<String> {
        // ... existing HTTP logic ...
    }
}
```

---

## Phase 4: Testing

### Step 7: Create Integration Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_websocket_connection() {
        // Note: Requires mock WebSocket server
        // For MVP, create mock_sn_utils_server test utility
        
        let client = SnUtilsWebSocketClient::new(
            "https://dev12345.service-now.com",
            "dev12345",
            "test_g_ck_token",
            "ws://127.0.0.1:1978",
        );

        // Would need mock server to test actual connection
        // For now, just verify client creation
        assert_eq!(client.instance.name, "dev12345");
    }

    #[test]
    fn test_rest_to_sn_utils_translation() {
        let (action, payload) = websocket_translator::rest_to_sn_utils_action(
            "GET",
            "/api/now/table/incident/INC123",
            None,
        ).unwrap();

        assert_eq!(action, "requestRecord");
        assert_eq!(payload["tableName"], "incident");
        assert_eq!(payload["sys_id"], "INC123");
    }
}
```

---

## Implementation Checklist

- [ ] Add `tokio-tungstenite` to `Cargo.toml`
- [ ] Create `src/auth/sn_utils_websocket.rs`
- [ ] Create `src/config/sn_utils_discovery.rs`
- [ ] Update `src/auth/mod.rs` with `AuthMethod::SnUtils`
- [ ] Create `src/client/websocket_translator.rs`
- [ ] Update `src/client/mod.rs` to route through WebSocket
- [ ] Add unit tests for translation layer
- [ ] Create mock WebSocket server for integration tests
- [ ] Document in config example
- [ ] Update CLI help text

---

## Next Steps

1. **Start with Phase 1**: Get WebSocket connection working
2. **MVP**: Implement `requestRecord` and `requestRecords` only
3. **Expand**: Add create/update/delete once core works
4. **Test**: Create mock server for reliable testing
5. **Document**: Write user guide for setup

See `docs/design/sn-scriptsync-integration.md` for full architectural details.
