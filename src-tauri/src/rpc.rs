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
        result.as_str().map(|s| s.to_string()).ok_or("Invalid network response".to_string())
    }

    pub async fn get_block_height(&self) -> Result<u64, String> {
        let result = self.call("chain_height", json!([])).await?;
        result.as_u64().ok_or("Invalid height response".to_string())
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
        result.as_bool().ok_or("Invalid validation response".to_string())
    }
}
