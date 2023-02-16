#![allow(dead_code)]

use ckb_jsonrpc_types::{
    BlockNumber, Capacity, CellOutput, HeaderView, JsonBytes, OutPoint, Script, TransactionView,
    TxStatus, Uint32, Uint64,
};
use ckb_types::H256;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};

use std::{
    future::Future,
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

macro_rules! jsonrpc {
    ($method:expr, $self:ident, $return:ty$(, $params:ident$(,)?)*) => {{
        let old = $self.id.fetch_add(1, Ordering::AcqRel);
        let data = format!(
            r#"{{"id": {}, "jsonrpc": "2.0", "method": "{}", "params": {}}}"#,
            old,
            $method,
            serde_json::to_value(($($params,)*)).unwrap()
        );

        let req_json: serde_json::Value = serde_json::from_str(&data).unwrap();

        let c = $self.raw.post($self.ckb_uri.clone()).json(&req_json);
        async {
            let resp = c
                .send()
                .await
                .map_err::<io::Error, _>(|e| io::Error::new(io::ErrorKind::ConnectionAborted, format!("{:?}", e)))?;
            let output = resp
                .json::<jsonrpc_core::response::Output>()
                .await
                .map_err::<io::Error, _>(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{:?}", e)))?;

            match output {
                jsonrpc_core::response::Output::Success(success) => {
                    Ok(serde_json::from_value::<$return>(success.result).unwrap())
                }
                jsonrpc_core::response::Output::Failure(e) => {
                    Err(io::Error::new(io::ErrorKind::InvalidData, format!("{:?}", e)))
                }
            }
        }
    }}
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct IndexerTip {
    pub block_hash: H256,
    pub block_number: BlockNumber,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    Desc,
    Asc,
}

#[derive(Serialize, Deserialize)]
pub struct Pagination<T> {
    pub objects: Vec<T>,
    pub last_cursor: JsonBytes,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CellType {
    Input,
    Output,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCell {
    pub tx_hash: H256,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
    pub io_index: Uint32,
    pub io_type: CellType,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TxWithCells {
    pub tx_hash: H256,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
    pub cells: Vec<(CellType, Uint32)>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Tx {
    Ungrouped(TxWithCell),
    Grouped(TxWithCells),
}

impl Tx {
    pub fn tx_hash(&self) -> H256 {
        match self {
            Tx::Ungrouped(tx) => tx.tx_hash.clone(),
            Tx::Grouped(tx) => tx.tx_hash.clone(),
        }
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IndexerScriptSearchMode {
    /// Mode `prefix` search script with prefix
    Prefix,
    /// Mode `exact` search script with exact match
    Exact,
}

impl Default for IndexerScriptSearchMode {
    fn default() -> Self {
        Self::Prefix
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SearchKey {
    pub script: Script,
    pub script_type: ScriptType,
    pub script_search_mode: Option<IndexerScriptSearchMode>,
    pub filter: Option<SearchKeyFilter>,
    pub with_data: Option<bool>,
    pub group_by_transaction: Option<bool>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SearchKeyFilter {
    pub script: Option<Script>,
    pub script_len_range: Option<[Uint64; 2]>,
    pub output_data_len_range: Option<[Uint64; 2]>,
    pub output_capacity_range: Option<[Uint64; 2]>,
    pub block_range: Option<[BlockNumber; 2]>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScriptType {
    Lock,
    Type,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CellsCapacity {
    pub capacity: Capacity,
    pub block_hash: H256,
    pub block_number: BlockNumber,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cell {
    pub output: CellOutput,
    pub output_data: Option<JsonBytes>,
    pub out_point: OutPoint,
    pub block_number: BlockNumber,
    pub tx_index: Uint32,
}

#[derive(Clone)]
pub struct RpcClient {
    raw: Client,
    ckb_uri: Url,
    id: Arc<AtomicU64>,
}

impl RpcClient {
    pub fn new(ckb_uri: &str) -> Self {
        let ckb_uri = Url::parse(ckb_uri).expect("ckb uri, e.g. \"http://127.0.0.1:8114\"");

        RpcClient {
            raw: Client::new(),
            ckb_uri,
            id: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn get_transaction(
        &self,
        hash: &H256,
    ) -> impl Future<Output = Result<Option<TransactionView>, io::Error>> {
        #[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Debug)]
        struct TransactionWithStatusResponse {
            /// The transaction.
            pub transaction: Option<TransactionView>,
            /// The Transaction status.
            pub tx_status: TxStatus,
        }
        let task = jsonrpc!("get_transaction", self, TransactionWithStatusResponse, hash);
        async {
            let res = task.await?;
            Ok(res.transaction)
        }
    }

    pub fn get_header_by_number(
        &self,
        number: BlockNumber,
    ) -> impl Future<Output = Result<HeaderView, io::Error>> {
        jsonrpc!("get_header_by_number", self, HeaderView, number)
    }

    pub fn get_header(
        &self,
        hash: H256,
    ) -> impl Future<Output = Result<Option<HeaderView>, io::Error>> {
        jsonrpc!("get_header", self, Option<HeaderView>, hash)
    }

    pub fn get_indexer_tip(&self) -> impl Future<Output = Result<IndexerTip, io::Error>> {
        jsonrpc!("get_indexer_tip", self, IndexerTip)
    }

    pub fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: Uint32,
        after: Option<JsonBytes>,
    ) -> impl Future<Output = Result<Pagination<Tx>, io::Error>> {
        jsonrpc!(
            "get_transactions",
            self,
            Pagination<Tx>,
            search_key,
            order,
            limit,
            after
        )
    }

    pub fn get_cells(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: Uint32,
        after: Option<JsonBytes>,
    ) -> impl Future<Output = Result<Pagination<Cell>, io::Error>> {
        jsonrpc!(
            "get_cells",
            self,
            Pagination<Cell>,
            search_key,
            order,
            limit,
            after
        )
    }

    pub fn get_cells_capacity(
        &self,
        search_key: SearchKey,
    ) -> impl Future<Output = Result<Option<CellsCapacity>, io::Error>> {
        jsonrpc!(
            "get_cells_capacity",
            self,
            Option<CellsCapacity>,
            search_key,
        )
    }
}
