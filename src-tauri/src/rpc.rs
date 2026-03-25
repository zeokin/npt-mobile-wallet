use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub struct RpcClient {
    client: Client,
    url: String,
    auth_token: Option<String>,
}

#[derive(Serialize)]
struct RpcRequest {
    jsonrpc: &'static str,
    method: String,
    params: Value,
    id: u64,
}

#[derive(Deserialize)]
struct RpcResponse {
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcClient {
    pub fn new(url: &str, auth_token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");
        Self {
            client,
            url: url.to_string(),
            auth_token,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let req = RpcRequest {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
            id: 1,
        };

        let mut builder = self.client.post(&self.url);
        if let Some(token) = &self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }

        let resp = builder
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Connection failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP error: {}", resp.status()));
        }

        let rpc_resp: RpcResponse = resp
            .json()
            .await
            .map_err(|e| format!("Invalid response: {}", e))?;

        if let Some(err) = rpc_resp.error {
            return Err(format!("RPC error {}: {}", err.code, err.message));
        }

        rpc_resp.result.ok_or_else(|| "No result in response".to_string())
    }

    pub async fn test_connection(&self) -> Result<(String, u64), String> {
        let network = self.get_network().await?;
        let height = self.get_block_height().await?;
        Ok((network, height))
    }

    pub async fn get_network(&self) -> Result<String, String> {
        let result = self.call("node_network", json!([])).await?;
        // Response: {"network": "main"} or just "main"
        if let Some(s) = result.as_str() {
            return Ok(s.to_string());
        }
        if let Some(s) = result.get("network").and_then(|v| v.as_str()) {
            return Ok(s.to_string());
        }
        Err(format!("Invalid network response: {}", result))
    }

    pub async fn get_block_height(&self) -> Result<u64, String> {
        let result = self.call("chain_height", json!([])).await?;
        // Response: {"height": 12345} or just 12345
        if let Some(n) = result.as_u64() {
            return Ok(n);
        }
        if let Some(n) = result.get("height").and_then(|v| v.as_u64()) {
            return Ok(n);
        }
        Err(format!("Invalid height response: {}", result))
    }

    pub async fn get_balance(&self) -> Result<Value, String> {
        self.call("personal_getBalance", json!([])).await
    }

    pub async fn generate_address(&self, key_type: &str) -> Result<String, String> {
        let result = self.call("personal_generateAddress", json!([key_type])).await?;
        result.as_str().map(|s| s.to_string()).ok_or("Invalid address response".to_string())
    }

    pub async fn send(&self, address: &str, amount: &str, fee: &str) -> Result<Value, String> {
        self.call("personal_send", json!([address, amount, fee])).await
    }

    pub async fn incoming_history(&self) -> Result<Value, String> {
        self.call("personal_incomingHistory", json!([])).await
    }

    pub async fn outgoing_history(&self) -> Result<Value, String> {
        self.call("personal_outgoingHistory", json!([])).await
    }

    pub async fn unspent_utxos(&self) -> Result<Value, String> {
        self.call("personal_unspentUtxos", json!([])).await
    }

    pub async fn claim_utxo(&self, utxo_data: &str) -> Result<Value, String> {
        self.call("personal_claimUtxo", json!([utxo_data])).await
    }

    pub async fn validate_address(&self, address: &str) -> Result<bool, String> {
        let result = self.call("wallet_validateAddress", json!([address])).await?;
        // Response formats:
        // - bool: true/false
        // - object with addressType: {"addressType":"generation",...} means valid
        // - object with "valid" field
        if let Some(b) = result.as_bool() {
            return Ok(b);
        }
        if let Some(b) = result.get("valid").and_then(|v| v.as_bool()) {
            return Ok(b);
        }
        // If response has "addressType", the address is valid
        if result.get("addressType").is_some() || result.get("address_type").is_some() {
            return Ok(true);
        }
        // If we got any non-error response, address is valid
        if result.is_object() && !result.get("error").is_some() {
            return Ok(true);
        }
        Ok(false)
    }

    // ── Chain State Endpoints ────────────────────────────────────

    /// Get the current chain tip block (includes mutator set accumulator).
    /// Method: chain_tip
    pub async fn get_tip(&self) -> Result<Value, String> {
        self.call("chain_tip", json!([])).await
    }

    /// Restore membership proofs for spending UTXOs.
    /// Method: wallet_restoreMembershipProof
    pub async fn restore_membership_proof(
        &self,
        absolute_index_sets: &Value,
    ) -> Result<Value, String> {
        self.call("wallet_restoreMembershipProof", json!([absolute_index_sets])).await
    }

    /// Submit a locally-built transaction.
    /// Method: wallet_submitTransaction
    pub async fn submit_transaction(&self, transaction: &Value) -> Result<Value, String> {
        self.call("wallet_submitTransaction", json!([transaction])).await
    }

    // ── UTXO Scanning Endpoints ─────────────────────────────────

    /// Find blocks containing announcements matching our flags.
    /// Method: utxoindex_blockHeightsByFlags
    pub async fn block_heights_by_flags(
        &self,
        flags: &[neptune_cash::state::wallet::address::announcement_flag::AnnouncementFlag],
    ) -> Result<Vec<u64>, String> {
        let flags_json = serde_json::to_value(flags)
            .map_err(|e| format!("Serialize flags: {}", e))?;

        // Debug: log what we're sending
        eprintln!("[DEBUG] blockHeightsByFlags params: {}",
            serde_json::to_string_pretty(&json!([flags_json])).unwrap_or_default());

        let result = self.call("utxoindex_blockHeightsByFlags", json!([flags_json])).await?;

        // Response: {"block_heights": [1, 2, 3]} or {"blockHeights": [...]}
        let heights = result
            .get("block_heights")
            .or_else(|| result.get("blockHeights"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Invalid blockHeightsByFlags response: {}", result))?;

        heights
            .iter()
            .map(|v| v.as_u64().ok_or_else(|| "Invalid block height".to_string()))
            .collect()
    }

    /// Get transaction kernel for a block at given height.
    /// Method: archival_getBlockTransactionKernel
    pub async fn get_block_transaction_kernel(
        &self,
        height: u64,
    ) -> Result<Option<Value>, String> {
        let result = self
            .call(
                "archival_getBlockTransactionKernel",
                json!([{ "Height": height }]),
            )
            .await?;

        // Response: {"kernel": {...}} or {"kernel": null}
        let kernel = result.get("kernel").unwrap_or(&result);
        if kernel.is_null() {
            Ok(None)
        } else {
            Ok(Some(kernel.clone()))
        }
    }

    /// Check if a UTXO's bloom indices are set (likely spent).
    /// Method: archival_areBloomIndicesSet
    pub async fn are_bloom_indices_set(&self, absolute_index_set: &Value) -> Result<bool, String> {
        let result = self
            .call("archival_areBloomIndicesSet", json!([absolute_index_set]))
            .await?;

        // Response: {"are_set": true} or {"areSet": true}
        result
            .get("are_set")
            .or_else(|| result.get("areSet"))
            .and_then(|v| v.as_bool())
            .ok_or_else(|| format!("Invalid areBloomIndicesSet response: {}", result))
    }
}
