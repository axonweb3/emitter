#![allow(dead_code)]

use async_trait::async_trait;
use ckb_jsonrpc_types::{
    BlockNumber, BlockView, HeaderView, JsonBytes, TransactionView, TxStatus, Uint32,
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

use crate::{
    types::{Cell, CellsCapacity, IndexerTip, Order, Pagination, SearchKey, Tx},
    Rpc,
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

// Default implementation of ckb Rpc client
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

    pub fn new_with_client(ckb_uri: &str, raw: Client) -> Self {
        let ckb_uri = Url::parse(ckb_uri).expect("ckb uri, e.g. \"http://127.0.0.1:8114\"");

        RpcClient {
            raw,
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

    pub fn get_block_by_number(
        &self,
        number: BlockNumber,
    ) -> impl Future<Output = Result<BlockView, io::Error>> {
        jsonrpc!("get_block_by_number", self, BlockView, number)
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

#[async_trait]
impl Rpc for RpcClient {
    async fn get_transactions(
        &self,
        search_key: SearchKey,
        order: Order,
        limit: Uint32,
        after: Option<JsonBytes>,
    ) -> Result<Pagination<Tx>, io::Error> {
        self.get_transactions(search_key, order, limit, after).await
    }

    async fn get_transaction(&self, hash: &H256) -> Result<Option<TransactionView>, io::Error> {
        self.get_transaction(hash).await
    }

    async fn get_header_by_number(&self, number: BlockNumber) -> Result<HeaderView, io::Error> {
        self.get_header_by_number(number).await
    }

    async fn get_indexer_tip(&self) -> Result<IndexerTip, io::Error> {
        self.get_indexer_tip().await
    }

    async fn get_block_by_number(&self, number: BlockNumber) -> Result<BlockView, std::io::Error> {
        self.get_block_by_number(number).await
    }
}
