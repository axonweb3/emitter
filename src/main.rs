use ckb_jsonrpc_types::{CellInfo, HeaderView, OutPoint};
use jsonrpsee::{core::async_trait, server::ServerBuilder};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use std::sync::{
    atomic::{AtomicPtr, Ordering},
    Arc,
};

use global_state::GlobalState;
use rpc_client::{IndexerTip, RpcClient};
use rpc_server::{EmitterRpc, EmitterServer};

mod cell_process;
mod global_state;
mod header_sync;
mod rpc_client;
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
        );

        let state = global.state.clone();

        global.spawn_header_sync(client.clone());

        let cell_handles = global.spawn_cells(client.clone());

        let _global_handle = tokio::spawn(async move { global.run().await });

        let rpc = EmitterRpc {
            state,
            cell_handles,
            client,
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

async fn submit_cells(submits: Vec<Submit>) {
    for sub in submits {
        println!("{}", serde_json::to_string_pretty(&sub).unwrap())
    }
}

async fn submit_headers(headers: Vec<HeaderView>) {
    for header in headers {
        println!("{}", serde_json::to_string_pretty(&header).unwrap())
    }
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
