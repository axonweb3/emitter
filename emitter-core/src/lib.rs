macro_rules! rpc_get {
    ($method:expr) => {{
        use std::io::ErrorKind::*;
        loop {
            match $method.await {
                Ok(r) => break r,
                Err(e) => match e.kind() {
                    ConnectionRefused | ConnectionReset | ConnectionAborted | BrokenPipe => {
                        panic!("{e}")
                    }
                    _ => (),
                },
            }
        }
    }};
}

pub mod cell_process;
pub mod header_sync;
#[cfg(feature = "client")]
pub mod rpc_client;
pub mod types;

use async_trait::async_trait;
use ckb_jsonrpc_types::{
    BlockNumber, BlockView, CellInfo, HeaderView, JsonBytes, OutPoint, TransactionView, Uint32,
};
use ckb_types::H256;
use serde::{Deserialize, Serialize};
use types::{HeaderViewWithExtension, IndexerTip, Order, Pagination, SearchKey, Tx};

// Cell changes on a single block
#[derive(Serialize, Deserialize)]
pub struct Submit {
    pub header: HeaderView,
    pub inputs: Vec<OutPoint>,
    pub outputs: Vec<(OutPoint, CellInfo)>,
}

pub trait TipState {
    fn load(&self) -> &IndexerTip;
    fn update(&mut self, current: IndexerTip);
}

#[async_trait]
pub trait SubmitProcess {
    fn is_closed(&self) -> bool;
    // if false return, it means this cell process should be shutdown
    async fn submit_cells(&mut self, cells: Vec<Submit>) -> bool;
    async fn submit_headers(&mut self, headers: Vec<HeaderViewWithExtension>) -> bool;
}

#[async_trait]
pub trait Rpc {
    // ckb indexer `get_transactions`
    async fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: Uint32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Tx>, std::io::Error>;

    // ckb rpc `get_transaction`
    async fn get_transaction(&self, hash: &H256)
        -> Result<Option<TransactionView>, std::io::Error>;

    // ckb rpc `get_header_by_number`
    async fn get_header_by_number(&self, number: BlockNumber)
        -> Result<HeaderView, std::io::Error>;
    // ckb indexer `get_indexer_tip`
    async fn get_indexer_tip(&self) -> Result<IndexerTip, std::io::Error>;

    // ckb rpc `get_block_by_number`
    async fn get_block_by_number(&self, number: BlockNumber) -> Result<BlockView, std::io::Error>;
}

impl TipState for IndexerTip {
    fn load(&self) -> &IndexerTip {
        self
    }

    fn update(&mut self, current: IndexerTip) {
        *self = current
    }
}
