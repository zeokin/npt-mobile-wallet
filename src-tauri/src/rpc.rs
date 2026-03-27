// JSON-RPC 2.0 client for neptune-core supporter node.

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
struct RpcRequest { jsonrpc: &'static str, method: String, params: Value, id: u64 }

#[derive(Deserialize)]
struct RpcResponse { result: Option<Value>, error: Option<RpcError> }

#[derive(Deserialize)]
struct RpcError { code: i64, message: String }

impl RpcClient {
    pub fn new(url: &str, auth_token: Option<String>) -> Self {
        Self {
            client: Client::builder().timeout(Duration::from_secs(300)).build().expect("HTTP client"),
            url: url.to_string(),
            auth_token,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let req = RpcRequest { jsonrpc: "2.0", method: method.to_string(), params, id: 1 };
        let mut builder = self.client.post(&self.url);
        if let Some(token) = &self.auth_token {
            builder = builder.header("Authorization", format!("Bearer {}", token));
        }
        let resp = builder.json(&req).send().await.map_err(|e| format!("Connection failed: {}", e))?;
        if !resp.status().is_success() { return Err(format!("HTTP error: {}", resp.status())); }
        let rpc_resp: RpcResponse = resp.json().await.map_err(|e| format!("Invalid response: {}", e))?;
        if let Some(err) = rpc_resp.error {
            return Err(format!("RPC error {}: {}", err.code, err.message));
        }
        rpc_resp.result.ok_or_else(|| "No result".to_string())
    }

    pub async fn test_connection(&self) -> Result<(String, u64), String> {
        Ok((self.get_network().await?, self.get_block_height().await?))
    }

    pub async fn get_network(&self) -> Result<String, String> {
        let r = self.call("node_network", json!([])).await?;
        r.as_str().map(|s| s.to_string())
            .or_else(|| r.get("network").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .ok_or_else(|| format!("Invalid network response: {}", r))
    }

    pub async fn get_block_height(&self) -> Result<u64, String> {
        let r = self.call("chain_height", json!([])).await?;
        r.as_u64()
            .or_else(|| r.get("height").and_then(|v| v.as_u64()))
            .ok_or_else(|| format!("Invalid height response: {}", r))
    }

    pub async fn get_tip(&self) -> Result<Value, String> {
        self.call("chain_tip", json!([])).await
    }

    pub async fn validate_address(&self, address: &str) -> Result<bool, String> {
        let r = self.call("wallet_validateAddress", json!([address])).await?;
        if let Some(b) = r.as_bool() { return Ok(b); }
        if r.get("addressType").is_some() || r.get("address_type").is_some() { return Ok(true); }
        if r.is_object() && r.get("error").is_none() { return Ok(true); }
        Ok(false)
    }

    pub async fn get_wallet_blocks(&self, from: u64, to: u64) -> Result<Value, String> {
        use neptune_cash::application::json_rpc::core::model::message::GetBlocksRequest;
        use neptune_cash::protocol::consensus::block::block_height::BlockHeight;
        let params = serde_json::to_value(&GetBlocksRequest {
            from_height: BlockHeight::from(from), to_height: BlockHeight::from(to),
        }).map_err(|e| format!("{}", e))?;
        self.call("wallet_getBlocks", params).await
    }

    pub async fn restore_membership_proof(&self, params: &Value) -> Result<Value, String> {
        self.call("wallet_restoreMembershipProof", params.clone()).await
    }

    pub async fn submit_transaction(&self, params: &Value) -> Result<Value, String> {
        self.call("wallet_submitTransaction", params.clone()).await
    }

    pub async fn was_mined(&self, params: &Value) -> Result<Value, String> {
        self.call("utxoindex_wasMined", params.clone()).await
    }

    pub async fn block_heights_by_flags(
        &self,
        flags: &[neptune_cash::state::wallet::address::announcement_flag::AnnouncementFlag],
    ) -> Result<Vec<u64>, String> {
        use neptune_cash::application::json_rpc::core::model::message::BlockHeightsByFlagsRequest;
        let params = serde_json::to_value(&BlockHeightsByFlagsRequest {
            announcement_flags: flags.to_vec(),
        }).map_err(|e| format!("{}", e))?;

        let result = self.call("utxoindex_blockHeightsByFlags", params).await?;
        result.get("blockHeights").or_else(|| result.get("block_heights"))
            .and_then(|v| v.as_array())
            .ok_or_else(|| format!("Invalid blockHeightsByFlags response: {}", result))?
            .iter().map(|v| v.as_u64().ok_or_else(|| "Invalid height".to_string()))
            .collect()
    }

    pub async fn get_block_transaction_kernel(&self, height: u64) -> Result<Option<Value>, String> {
        use neptune_cash::application::json_rpc::core::model::message::GetBlockTransactionKernelRequest;
        use neptune_cash::protocol::consensus::block::block_selector::BlockSelector;
        use neptune_cash::protocol::consensus::block::block_height::BlockHeight;
        let params = serde_json::to_value(&GetBlockTransactionKernelRequest {
            selector: BlockSelector::Height(BlockHeight::from(height)),
        }).map_err(|e| format!("{}", e))?;

        let result = self.call("archival_getBlockTransactionKernel", params).await?;
        let kernel = result.get("kernel").unwrap_or(&result);
        Ok(if kernel.is_null() { None } else { Some(kernel.clone()) })
    }

    pub async fn are_bloom_indices_set(&self, abs_set: &Value) -> Result<bool, String> {
        let r = self.call("archival_areBloomIndicesSet", json!([abs_set])).await?;
        r.get("areSet").or_else(|| r.get("are_set"))
            .and_then(|v| v.as_bool())
            .ok_or_else(|| format!("Invalid response: {}", r))
    }
}
