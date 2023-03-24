mod emit_data;
mod global_state;
mod rpc_server;
mod ws_subscription;

use async_trait::async_trait;
use emitter_core::{
    rpc_client::RpcClient,
    types::{HeaderViewWithExtension, IndexerTip},
    Submit, SubmitProcess, TipState,
};
use jsonrpsee::server::ServerBuilder;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

use crate::{
    emit_data::eth_tx::{send_eth_tx, IMAGE_CELL_ADDRESS},
    emit_data::tx_data::convert_blocks,
    global_state::GlobalState,
    rpc_server::{EmitterRpc, EmitterServer},
};
use emit_data::eth_tx::{send_eth_tx, IMAGE_CELL_ADDRESS};
use emit_data::tx_data::convert_blocks;
use global_state::GlobalState;
use rpc_client::{IndexerTip, RpcClient};
use rpc_server::{EmitterRpc, EmitterServer};

mod emit_data;
mod global_state;
mod rpc_server;
mod ws_subscription;

#[tokio::main]
async fn main() {
    env_logger::init();

    let cmd = clap::Command::new("emitter")
    .version(clap::crate_version!()).arg(
        clap::Arg::new("ckb_uri")
            .short('c')
            .default_value("http://127.0.0.1:8114")
            .help(
                "CKB rpc service uri, supports http and tcp, for example: `http://127.0.0.1:8114`",
            )
            .action(clap::ArgAction::Set),
    ).arg(
        clap::Arg::new("listen_uri")
        .short('l')
        .default_value("127.0.0.1:8120")
        .help("Emitter rpc http service listen address, default 127.0.0.1:8120")
        .action(clap::ArgAction::Set),
    ).arg(
        clap::Arg::new("store_path")
        .short('s')
        .help("Sets the indexer store path to use")
        .required(true)
        .action(clap::ArgAction::Set),
    ).arg(
        clap::Arg::new("axon_uri")
        .long("i")
        .default_value("http://127.0.0.1:8080")
        .help("The Axon listening address, default http://127.0.0.1:8080")
        .action(clap::ArgAction::Set)
    )
    .arg(
        clap::Arg::new("ws")
        .long("ws")
        .conflicts_with("store_path")
        .help("Use ws mode instead of http rpc mode, ws only has subscription mode")
        .action(clap::ArgAction::SetTrue)
    );

    let matches = cmd.get_matches();

    let client = RpcClient::new(matches.get_one::<String>("ckb_uri").unwrap());

    let listen_url = matches.get_one::<String>("listen_uri").unwrap();
    if matches.get_flag("ws") {
        let rpc = ws_subscription::ws_subscription_module(client).await;
        let handle = ServerBuilder::new()
            .ws_only()
            .build(listen_url)
            .await
            .unwrap()
            .start(rpc)
            .unwrap();
        log::info!("websocket listen on {}", listen_url);
        handle.stopped().await;
    } else {
        let genesis = client.get_header_by_number(0.into()).await.unwrap();

        let mut global = GlobalState::new(
            matches.get_one::<String>("store_path").unwrap().into(),
            genesis,
            matches.get_one::<String>("axon_uri").unwrap().into(),
        );

        let state = global.state.clone();

        global.spawn_header_sync(client.clone());

        let cell_handles = global.spawn_cells(client.clone());

        let _global_handle = tokio::spawn(async move { global.run().await });

        let rpc = EmitterRpc {
            state,
            cell_handles,
            client,
            axon_url: matches.get_one::<String>("axon_uri").unwrap().into(),
        }
        .into_rpc();

        let handle = ServerBuilder::new()
            .http_only()
            .build(listen_url)
            .await
            .unwrap()
            .start(rpc)
            .unwrap();

        log::info!("listen on {}", listen_url);
        handle.stopped().await;
    }
}

#[derive(Serialize, Deserialize)]
pub struct Submit {
    header: HeaderView,
    inputs: Vec<OutPoint>,
    outputs: Vec<(OutPoint, CellInfo)>,
}

async fn submit_cells(axon_url: &str, submits: Vec<Submit>) {
    // for sub in submits.iter() {
    //     println!("{}", serde_json::to_string_pretty(sub).unwrap());
    // }

    if let Err(e) = send_eth_tx(axon_url, convert_blocks(submits), IMAGE_CELL_ADDRESS).await {
        println!("emitter submit tx error: {e}")
    };
}

async fn submit_headers(_headers: Vec<HeaderView>) {
    // for header in headers {
    //     println!("{}", serde_json::to_string_pretty(&header).unwrap())
    // }
}

#[async_trait]
pub(crate) trait SubmitProcess {
    fn is_closed(&self) -> bool;
    // if false return, it means this cell process should be shutdown
    async fn submit_cells(&mut self, cells: Vec<Submit>) -> bool;
    async fn submit_headers(&mut self, headers: Vec<HeaderView>) -> bool;
}

pub(crate) trait TipState {
    fn load(&self) -> &IndexerTip;
    fn update(&mut self, current: IndexerTip);
}

struct ScanTipInner(AtomicPtr<IndexerTip>);

pub struct ScanTip(Arc<ScanTipInner>);

impl Drop for ScanTipInner {
    fn drop(&mut self) {
        unsafe { drop(Box::from_raw(self.0.load(Ordering::Relaxed))) }
    }
}

impl Clone for ScanTip {
    fn clone(&self) -> Self {
        ScanTip(self.0.clone())
    }
}

impl TipState for ScanTip {
    fn load(&self) -> &IndexerTip {
        unsafe { &*self.0 .0.load(Ordering::Acquire) }
    }

    fn update(&mut self, current: IndexerTip) {
        let raw = self
            .0
             .0
            .swap(Box::into_raw(Box::new(current)), Ordering::AcqRel);

        unsafe {
            drop(Box::from_raw(raw));
        }
    }
}

impl Serialize for ScanTip {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let inner = unsafe { &*self.0 .0.load(Ordering::Acquire) };

        inner.serialize(serializer)
    }
}

impl<'a> Deserialize<'a> for ScanTip {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        let inner = IndexerTip::deserialize(deserializer)?;

        Ok(ScanTip(Arc::new(ScanTipInner(AtomicPtr::new(
            Box::into_raw(Box::new(inner)),
        )))))
    }
}

async fn submit_cells(submits: Vec<Submit>) {
    for sub in submits {
        println!("{}", serde_json::to_string_pretty(&sub).unwrap())
    }
}

async fn submit_headers(headers: Vec<HeaderViewWithExtension>) {
    for header in headers {
        println!("{}", serde_json::to_string_pretty(&header).unwrap())
    }
}

pub(crate) struct RpcSubmit {
    pub axon_url: String,
}

#[async_trait]
impl SubmitProcess for RpcSubmit {
    fn is_closed(&self) -> bool {
        false
    }

    async fn submit_cells(&mut self, cells: Vec<Submit>) -> bool {
        submit_cells(&self.axon_url, cells).await;
        true
    }

    async fn submit_headers(&mut self, headers: Vec<HeaderViewWithExtension>) -> bool {
        submit_headers(headers).await;
        true
    }
}
