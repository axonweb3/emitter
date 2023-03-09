use ckb_jsonrpc_types::{BlockNumber, Script, Uint64};
use jsonrpsee::{
    core::{async_trait, Error},
    proc_macros::rpc,
};
use serde::{Deserialize, Serialize};

use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

use crate::{
    cell_process::{CellProcess, RpcSubmit},
    global_state::State,
    rpc_client::{
        IndexerScriptSearchMode, IndexerTip, RpcClient, ScriptType, SearchKey, SearchKeyFilter,
    },
    ScanTip, ScanTipInner,
};

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct RpcSearchKey {
    pub script: Script,
    pub script_type: ScriptType,
    pub script_search_mode: Option<IndexerScriptSearchMode>,
    pub filter: Option<RpcSearchKeyFilter>,
}

impl RpcSearchKey {
    pub fn into_key(self, block_range: Option<[Uint64; 2]>) -> SearchKey {
        SearchKey {
            script: self.script,
            script_type: self.script_type,
            filter: if self.filter.is_some() {
                self.filter.map(|f| f.into_filter(block_range))
            } else {
                Some(RpcSearchKeyFilter::default().into_filter(block_range))
            },
            script_search_mode: self.script_search_mode,
            with_data: None,
            group_by_transaction: Some(true),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, Hash, PartialEq, Eq)]
pub struct RpcSearchKeyFilter {
    pub script: Option<Script>,
    pub script_len_range: Option<[Uint64; 2]>,
    pub output_data_len_range: Option<[Uint64; 2]>,
    pub output_capacity_range: Option<[Uint64; 2]>,
}

impl RpcSearchKeyFilter {
    fn into_filter(self, block_range: Option<[Uint64; 2]>) -> SearchKeyFilter {
        SearchKeyFilter {
            script: self.script,
            script_len_range: self.script_len_range,
            output_data_len_range: self.output_data_len_range,
            output_capacity_range: self.output_capacity_range,
            block_range,
        }
    }
}

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

            let mut cell_process = CellProcess {
                key: search_key.clone(),
                client: self.client.clone(),
                scan_tip,
                process_fn: RpcSubmit,
                stop: false,
            };

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
            unsafe { (&*self.state.header_state.0 .0.load(Ordering::Acquire)).block_number };
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
