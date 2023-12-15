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
    emit_data::eth_tx::{send_eth_tx, wallet, CKB_LIGHT_CLIENT_ADDRESS, IMAGE_CELL_ADDRESS},
    emit_data::tx_data::{convert_blocks, convert_headers},
    global_state::GlobalState,
    rpc_server::{EmitterRpc, EmitterServer},
};

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
        clap::Arg::new("private_path")
        .short('p')
        .help("Sets the private key path to use")
        .help("The Axon trasaction signer key, use to construct transaction, default is axon demo wallet")
        .action(clap::ArgAction::Set),
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
    if let Some(priv_path) = matches.get_one::<String>("private_path") {
        load_privkey_from_file(priv_path);
    }
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

async fn submit_cells(axon_url: &str, submits: Vec<Submit>) {
    if let Err(e) = send_eth_tx(axon_url, convert_blocks(submits), IMAGE_CELL_ADDRESS).await {
        println!("emitter submit cells tx error: {e}")
    };
}

async fn submit_headers(axon_url: &str, headers: Vec<HeaderViewWithExtension>) {
    if let Err(e) = send_eth_tx(axon_url, convert_headers(headers), CKB_LIGHT_CLIENT_ADDRESS).await
    {
        println!("emitter submit headers tx error: {e}")
    };
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
        let new_number = current.block_number;
        let new_ptr = Box::into_raw(Box::new(current));
        if let Ok(raw) = self
            .0
             .0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |raw| {
                if unsafe { (*raw).block_number } < new_number {
                    Some(new_ptr)
                } else {
                    None
                }
            })
        {
            unsafe {
                drop(Box::from_raw(raw));
            }
        } else {
            unsafe { drop(Box::from_raw(new_ptr)) }
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
        submit_headers(&self.axon_url, headers).await;
        true
    }
}

fn load_privkey_from_file(privkey_path: &str) {
    use std::io::Read;
    let privkey = std::fs::File::open(privkey_path)
        .and_then(|mut f| {
            let mut buffer = Vec::new();
            f.read_to_end(&mut buffer).map(|_| buffer)
        })
        .expect("failed to parse private key file");
    wallet(Some(&privkey));
}
