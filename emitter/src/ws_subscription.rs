use ckb_jsonrpc_types::BlockNumber;
use emitter_core::{
    cell_process::CellProcess,
    header_sync::HeaderSyncProcess,
    rpc_client::RpcClient,
    types::{HeaderViewWithExtension, IndexerTip, RpcSearchKey},
    Submit, SubmitProcess,
};
use jsonrpsee::{
    core::async_trait,
    server::{RpcModule, SubscriptionSink},
    types::error::CallError,
};

use std::io;

struct WsSubmit(SubscriptionSink);

#[async_trait]
impl SubmitProcess for WsSubmit {
    fn is_closed(&self) -> bool {
        self.0.is_closed()
    }

    async fn submit_cells(&mut self, cells: Vec<Submit>) -> bool {
        if cells.is_empty() {
            return true;
        }
        match self.0.send(&cells) {
            Ok(r) => r,
            Err(e) => {
                log::error!("submit cells error: {}", e);
                false
            }
        }
    }

    async fn submit_headers(&mut self, headers: Vec<HeaderViewWithExtension>) -> bool {
        if headers.is_empty() {
            return true;
        }
        match self.0.send(&headers) {
            Ok(r) => r,
            Err(e) => {
                log::error!("submit headers error: {}", e);
                false
            }
        }
    }
}

pub async fn ws_subscription_module(client: RpcClient) -> RpcModule<RpcClient> {
    let mut rpc = RpcModule::new(client);

    rpc.register_subscription(
        "emitter_subscription",
        "emitter_subscription",
        "emitter_unsubscribe",
        |params, mut sink, ctx| {
            sink.accept()?;
            let mut iter = params.sequence();
            let method: &str = iter.next()?;

            match method {
                "cell_filter" => {
                    let key: RpcSearchKey = iter.next()?;
                    let start: BlockNumber = iter.next()?;
                    let client = ctx.as_ref().clone();

                    tokio::spawn(async move {
                        match cell_process(start, client.clone()).await {
                            Ok(tip) => {
                                let mut cell_process =
                                    CellProcess::new(key, tip, client, WsSubmit(sink));

                                tokio::spawn(async move {
                                    cell_process.run().await;
                                });
                            }
                            Err(e) => {
                                let _ = sink.reject(e);
                            }
                        }
                    });
                }
                "header_sync" => {
                    let start: BlockNumber = iter.next()?;
                    let client = ctx.as_ref().clone();
                    tokio::spawn(async move {
                        let start_tip = {
                            let header = client.get_header_by_number(start).await.unwrap();
                            IndexerTip {
                                block_hash: header.hash,
                                block_number: header.inner.number,
                            }
                        };
                        let mut header_sync =
                            HeaderSyncProcess::new(start_tip, client, WsSubmit(sink));
                        header_sync.run().await;
                    });
                }
                _ => {
                    return Err(CallError::from_std_error(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid method: {}", method),
                    ))
                    .into());
                }
            }

            Ok(())
        },
    )
    .unwrap();
    rpc
}

async fn cell_process(start: BlockNumber, client: RpcClient) -> Result<IndexerTip, CallError> {
    let indexer_tip = client.get_indexer_tip().await.map_err(|e| {
        CallError::from_std_error(io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
    })?;

    if indexer_tip.block_number > start {
        let header = client.get_header_by_number(start).await.map_err(|e| {
            CallError::from_std_error(io::Error::new(io::ErrorKind::InvalidData, e.to_string()))
        })?;

        let tip = IndexerTip {
            block_hash: header.hash,
            block_number: header.inner.number,
        };
        Ok(tip)
    } else {
        Err(CallError::from_std_error(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid start block number".to_string(),
        )))
    }
}
