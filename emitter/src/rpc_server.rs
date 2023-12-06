use ckb_jsonrpc_types::BlockNumber;
use emitter_core::{
    cell_process::CellProcess,
    rpc_client::RpcClient,
    types::{IndexerTip, RpcSearchKey},
};
use jsonrpsee::{
    core::{async_trait, Error},
    proc_macros::rpc,
};

use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

use crate::{global_state::State, RpcSubmit, ScanTip, ScanTipInner};

#[rpc(server)]
pub trait Emitter {
    #[method(name = "register")]
    async fn register(&self, search_key: RpcSearchKey, start: BlockNumber) -> Result<bool, Error>;

    #[method(name = "delete")]
    async fn delete(&self, search_key: RpcSearchKey) -> Result<bool, Error>;

    #[method(name = "info")]
    async fn info(&self) -> Result<State, Error>;

    #[method(name = "header_sync_start")]
    async fn header_sync_start(&self, number: BlockNumber) -> Result<bool, Error>;
}

pub(crate) struct EmitterRpc {
    pub state: State,
    pub cell_handles: Arc<dashmap::DashMap<RpcSearchKey, tokio::task::JoinHandle<()>>>,
    pub client: RpcClient,
    pub axon_url: String,
}

#[async_trait]
impl EmitterServer for EmitterRpc {
    async fn register(&self, search_key: RpcSearchKey, start: BlockNumber) -> Result<bool, Error> {
        if self.state.cell_states.contains_key(&search_key) {
            return Ok(false);
        }
        let indexer_tip = self
            .client
            .get_indexer_tip()
            .await
            .map_err(|e| Error::Custom(e.to_string()))?;

        if indexer_tip.block_number > start {
            let header = self
                .client
                .get_header_by_number(start)
                .await
                .map_err(|e| Error::Custom(e.to_string()))?;

            let scan_tip = {
                let tip = IndexerTip {
                    block_hash: header.hash,
                    block_number: header.inner.number,
                };
                ScanTip(Arc::new(ScanTipInner(AtomicPtr::new(Box::into_raw(
                    Box::new(tip),
                )))))
            };

            self.state
                .cell_states
                .insert(search_key.clone(), scan_tip.clone());

            let mut cell_process = CellProcess::new(
                search_key.clone(),
                scan_tip,
                self.client.clone(),
                RpcSubmit {
                    axon_url: self.axon_url.clone(),
                },
            );

            let handle = tokio::spawn(async move {
                cell_process.run().await;
            });

            self.cell_handles.insert(search_key, handle);
            return Ok(true);
        }

        Ok(false)
    }

    async fn delete(&self, search_key: RpcSearchKey) -> Result<bool, Error> {
        if self.state.cell_states.remove(&search_key).is_some() {
            if let Some(handle) = self.cell_handles.get(&search_key) {
                handle.abort();
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn info(&self) -> Result<State, Error> {
        Ok(self.state.clone())
    }

    async fn header_sync_start(&self, number: BlockNumber) -> Result<bool, Error> {
        let current =
            unsafe { (*self.state.header_state.0 .0.load(Ordering::Acquire)).block_number };
        if number < current {
            Ok(false)
        } else {
            let new_header = self.client.get_header_by_number(number).await?;
            let new_tip = {
                IndexerTip {
                    block_hash: new_header.hash,
                    block_number: new_header.inner.number,
                }
            };
            let raw = self
                .state
                .header_state
                .0
                 .0
                .swap(Box::into_raw(Box::new(new_tip)), Ordering::AcqRel);

            unsafe {
                drop(Box::from_raw(raw));
            }
            Ok(true)
        }
    }
}
