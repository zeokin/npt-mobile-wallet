//! JSON-RPC transport for neptune-core supporter nodes.
//!
//! We implement neptune-rpc-api's [`Transport`] trait. Because neptune-rpc-api
//! provides a blanket `impl<T: Transport> RpcApi for T`, implementing
//! `Transport` gives us every typed RPC method (`tip_header`, `height`,
//! `network`, `get_blocks`, `was_mined`, `restore_membership_proof`,
//! `submit_transaction`, `block_heights_by_flags`,
//! `batch_are_bloom_indices_set`, ...) for free — no hand-rolled
//! request/response parsing required. Server-side errors that carry a typed
//! payload (e.g. `SubmitTransactionError::NotConfirmable`) are decoded into
//! [`neptune_rpc_api::api::rpc::RpcError`] by that blanket impl.
//!
//! Equivalent to the official `neptune-rpc-client` crate, but built on
//! reqwest with `rustls-tls` so we don't pull OpenSSL into Android builds.

use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use neptune_rpc_api::api::client::transport::Transport;
use neptune_rpc_api::api::rpc::RpcApi;
use neptune_rpc_api::model::json::JsonError;
use neptune_rpc_api::model::json::JsonRequest;
use neptune_rpc_api::model::json::JsonResponse;
use neptune_rpc_api::model::json::JsonResult;
use reqwest::Client;
use serde_json::Value;

/// Time allowed to establish the TCP/TLS connection. A supporter that does not
/// answer within this window is treated as offline instead of leaving the UI
/// on "Connecting…" for minutes.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default per-request timeout for small calls (height, flags, headers, …).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Per-request timeout for calls that move a lot of data or make the node do
/// a lot of work (block downloads, membership proofs, transaction upload).
const LONG_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Clone)]
pub(crate) struct RpcClient {
    url: String,
    client: Client,
    id_counter: Arc<AtomicU64>,
}

impl RpcClient {
    pub(crate) fn new(url: &str) -> Result<Self, String> {
        let url = url.trim().trim_end_matches('/').to_string();
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err("Node URL must start with https:// or http://".to_string());
        }
        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(LONG_TIMEOUT)
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {e}"))?;
        Ok(Self {
            url,
            client,
            id_counter: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Ping the node: returns (network, tip height). Any RPC error is
    /// converted to a String so call sites don't need to match on RpcError.
    pub(crate) async fn test_connection(&self) -> Result<(String, u64), String> {
        let network = self
            .network()
            .await
            .map_err(|e| format!("Cannot reach node: {}", e))?;
        let height = self
            .height()
            .await
            .map_err(|e| format!("Cannot reach node: {}", e))?;
        Ok((network.network, u64::from(height.height)))
    }

    fn timeout_for(method: &str) -> Duration {
        match method {
            "wallet_submitTransaction"
            | "wallet_restoreMembershipProof"
            | "wallet_getBlocks"
            | "archival_getBlock"
            | "archival_getBlockTransactionKernel" => LONG_TIMEOUT,
            _ => DEFAULT_TIMEOUT,
        }
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
            .timeout(Self::timeout_for(method))
            .json(&req)
            .send()
            .await
            .map_err(|e| JsonError::ConnectionFailed {
                message: if e.is_timeout() {
                    "the node did not answer in time".to_string()
                } else if e.is_connect() {
                    "could not connect to the node".to_string()
                } else {
                    e.to_string()
                },
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            let snippet: String = body.chars().take(160).collect();
            return Err(JsonError::HttpError {
                message: if snippet.is_empty() {
                    status.to_string()
                } else {
                    format!("{status}: {snippet}")
                },
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
