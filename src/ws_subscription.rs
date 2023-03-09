use ckb_types::H256;
use jsonrpsee::{
    core::async_trait,
    types::error::CallError,
    ws_server::{RpcModule, SubscriptionSink},
};

use std::{collections::HashMap, io};

use crate::rpc_client::{IndexerTip, RpcClient};
use crate::rpc_server::RpcSearchKey;
use crate::{
    cell_process::{CellProcess, SubmitProcess, TipState},
    Submit,
};
use ckb_jsonrpc_types::BlockNumber;

impl TipState for IndexerTip {
    fn load(&self) -> &IndexerTip {
        &self
    }

    fn update(&mut self, current: IndexerTip) {
        *self = current
    }
}

#[async_trait]
impl SubmitProcess for SubscriptionSink {
    fn is_closed(&self) -> bool {
        self.is_closed()
    }

    async fn submit(&mut self, cells: HashMap<H256, Submit>) -> bool {
        if cells.is_empty() {
            return true;
        }
        match self.send(&cells) {
            Ok(r) => r,
            Err(e) => {
                log::error!("submit error: {}", e);
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
                                let mut cell_process = CellProcess {
                                    key,
                                    client,
                                    scan_tip: tip,
                                    process_fn: sink,
                                    stop: false,
                                };

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
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(8));
                        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                        let mut start_tip = {
                            let header = client.get_header_by_number(start).await.unwrap();
                            IndexerTip {
                                block_hash: header.hash,
                                block_number: header.inner.number,
                            }
                        };
                        loop {
                            let indexer_tip = client.get_indexer_tip().await.unwrap();
                            if indexer_tip.block_number.value().saturating_sub(24)
                                > start_tip.block_number.value()
                            {
                                let new_tip = {
                                    let new = client
                                        .get_header_by_number(
                                            // 256 headers as a step
                                            std::cmp::min(
                                                indexer_tip.block_number.value().saturating_sub(24),
                                                start_tip.block_number.value() + 256,
                                            )
                                            .into(),
                                        )
                                        .await
                                        .unwrap();
                                    IndexerTip {
                                        block_hash: new.hash,
                                        block_number: new.inner.number,
                                    }
                                };

                                let mut headers = Vec::with_capacity(
                                    (new_tip.block_number.value() - start_tip.block_number.value())
                                        as usize,
                                );

                                for i in start_tip.block_number.value() + 1
                                    ..=new_tip.block_number.value()
                                {
                                    let header =
                                        client.get_header_by_number(i.into()).await.unwrap();
                                    headers.push(header);
                                }

                                match sink.send(&headers) {
                                    Ok(r) => {
                                        if !r {
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("submit error: {}", e);
                                        break;
                                    }
                                }

                                start_tip = new_tip;
                            } else {
                                interval.tick().await;
                            }
                        }
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
