//! JSON-RPC transport for neptune-core supporter nodes.
//!
//! We implement neptune-cash's [`Transport`] trait. Because neptune-cash
//! provides a blanket `impl<T: Transport> RpcApi for T`, implementing
//! `Transport` gives us every typed RPC method (`tip`, `height`, `network`,
//! `get_blocks`, `was_mined`, `restore_membership_proof`,
//! `submit_transaction`, `block_heights_by_flags`,
//! `block_heights_by_absolute_index_sets`, ...) for free — no hand-rolled
//! request/response parsing required.
//!
//! Equivalent to the official `neptune-rpc-client` crate, but built on
//! reqwest with `rustls-tls` so we don't pull OpenSSL into Android builds.

use async_trait::async_trait;
use neptune_cash::application::json_rpc::core::api::client::transport::Transport;
use neptune_cash::application::json_rpc::core::api::rpc::RpcApi;
use neptune_cash::application::json_rpc::core::model::json::{
    JsonError, JsonRequest, JsonResponse, JsonResult,
};
use reqwest::Client;
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct RpcClient {
    url: String,
    client: Client,
    id_counter: Arc<AtomicU64>,
}

impl RpcClient {
    pub(crate) fn new(url: &str, _auth_token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .expect("Failed to create HTTP client");
        Self {
            url: url.to_string(),
            client,
            id_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Ping the node: returns (network, tip height). Any RPC error is
    /// converted to a String so call sites don't need to match on RpcError.
    pub(crate) async fn test_connection(&self) -> Result<(String, u64), String> {
        let network = self
            .network()
            .await
            .map_err(|e| format!("network: {}", e))?;
        let height = self.height().await.map_err(|e| format!("height: {}", e))?;
        Ok((network.network, u64::from(height.height)))
    }
}

#[async_trait]
impl Transport for RpcClient {
    async fn call(&self, method: &str, params: Value) -> JsonResult<Value> {
        let req = JsonRequest {
            jsonrpc: Some("2.0".to_string()),
            method: method.to_string(),
            params,
            id: Some(self.id_counter.fetch_add(1, Ordering::SeqCst).into()),
        };

        let resp = self
            .client
            .post(&self.url)
            .json(&req)
            .send()
            .await
            .map_err(|e| JsonError::ConnectionFailed {
                message: e.to_string(),
            })?;

        if !resp.status().is_success() {
            return Err(JsonError::HttpError {
                message: resp.status().to_string(),
            });
        }

        let value: Value = resp.json().await.map_err(|_| JsonError::ParseError)?;
        let response: JsonResponse =
            serde_json::from_value(value).map_err(|_| JsonError::ParseError)?;

        match response {
            JsonResponse::Success { result, .. } => Ok(result),
            JsonResponse::Error { error, .. } => Err(error),
        }
    }
}
