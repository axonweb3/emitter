use ckb_jsonrpc_types::HeaderView;
use serde::{ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fs::{copy, create_dir_all, remove_file, rename, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicPtr, Ordering},
        Arc,
    },
};

use crate::{
    cell_process::CellProcess,
    rpc_client::{IndexerTip, RpcClient},
    rpc_server::RpcSearchKey,
    submit_headers, ScanTip, ScanTipInner,
};

#[derive(Clone)]
pub struct State {
    pub cell_states: Arc<dashmap::DashMap<RpcSearchKey, ScanTip>>,
    pub header_state: ScanTip,
}

impl Serialize for State {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("State", 2)?;
        state.serialize_field(
            "cell_states",
            &self
                .cell_states
                .iter()
                .map(|kv| (kv.key().clone(), kv.value().clone()))
                .collect::<Vec<_>>(),
        )?;
        state.serialize_field("header_state", &self.header_state)?;
        state.end()
    }
}

impl<'a> Deserialize<'a> for State {
    fn deserialize<D>(deserializer: D) -> Result<State, D::Error>
    where
        D: Deserializer<'a>,
    {
        #[derive(Deserialize, Serialize)]
        struct StateVisitor {
            cell_states: Vec<(RpcSearchKey, ScanTip)>,
            header_state: ScanTip,
        }

        let v: StateVisitor = Deserialize::deserialize(deserializer)?;
        Ok(State {
            cell_states: Arc::new(v.cell_states.into_iter().collect()),
            header_state: v.header_state,
        })
    }
}

pub(crate) struct GlobalState {
    pub state: State,
    path: PathBuf,
}

impl Drop for GlobalState {
    fn drop(&mut self) {
        self.dump_to_dir(self.path.clone())
    }
}

impl GlobalState {
    pub fn new(path: PathBuf, default_header: HeaderView) -> Self {
        let default_scan_tip = {
            let tip = IndexerTip {
                block_hash: default_header.hash,
                block_number: default_header.inner.number,
            };
            ScanTip(Arc::new(ScanTipInner(AtomicPtr::new(Box::into_raw(
                Box::new(tip),
            )))))
        };
        let state = Self::load_from_dir(path.clone(), default_scan_tip);

        Self { state, path }
    }

    pub async fn run(&mut self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;
            self.dump_to_dir(self.path.clone());
        }
    }

    pub fn spawn_cells(
        &self,
        client: RpcClient,
    ) -> dashmap::DashMap<RpcSearchKey, tokio::task::JoinHandle<()>> {
        let res = dashmap::DashMap::with_capacity(self.state.cell_states.len());
        if !self.state.cell_states.is_empty() {
            for kv in self.state.cell_states.iter() {
                let mut cell_process = CellProcess {
                    key: kv.key().clone(),
                    scan_tip: kv.value().clone(),
                    client: client.clone(),
                };

                let handle = tokio::spawn(async move {
                    cell_process.run().await;
                });
                res.insert(kv.key().clone(), handle);
            }
        }
        res
    }

    pub fn spawn_header_sync(&self, client: RpcClient) {
        let state = self.state.header_state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(8));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                let indexer_tip = client.get_indexer_tip().await.unwrap();
                let old_tip = unsafe { &*state.0 .0.load(Ordering::Acquire) }.clone();
                if indexer_tip.block_number.value().saturating_sub(24)
                    > old_tip.block_number.value()
                {
                    let new_tip = {
                        let new = client
                            .get_header_by_number(
                                // 256 headers as a step
                                std::cmp::min(
                                    indexer_tip.block_number.value().saturating_sub(24),
                                    old_tip.block_number.value() + 256,
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
                        (new_tip.block_number.value() - old_tip.block_number.value()) as usize,
                    );

                    for i in old_tip.block_number.value() + 1..=new_tip.block_number.value() {
                        let header = client.get_header_by_number(i.into()).await.unwrap();
                        headers.push(header);
                    }

                    submit_headers(headers).await;

                    let raw = state
                        .0
                         .0
                        .swap(Box::into_raw(Box::new(new_tip)), Ordering::AcqRel);

                    unsafe {
                        drop(Box::from_raw(raw));
                    }
                } else {
                    interval.tick().await;
                }
            }
        });
    }

    fn load_from_dir(path: PathBuf, default_scan_tip: ScanTip) -> State {
        let db_path = path.join("scan_state");

        match File::open(&db_path) {
            Ok(f) => serde_json::from_reader(f).unwrap_or(State {
                cell_states: Default::default(),
                header_state: default_scan_tip,
            }),
            Err(e) => {
                log::warn!(
                    "Failed to open state db, file: {:?}, error: {:?}",
                    db_path,
                    e
                );
                State {
                    cell_states: Default::default(),
                    header_state: default_scan_tip,
                }
            }
        }
    }

    fn dump_to_dir<P: AsRef<Path>>(&self, path: P) {
        // create dir
        create_dir_all(&path).unwrap();
        // dump file to a temporary sub-directory
        let tmp_dir = path.as_ref().join("tmp");
        create_dir_all(&tmp_dir).unwrap();
        let tmp_scan_state = tmp_dir.join("scan_state");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .append(false)
            .open(&tmp_scan_state)
            .unwrap();
        // empty file and dump the json string to it
        file.set_len(0)
            .and_then(|_| serde_json::to_string(&self.state).map_err(Into::into))
            .and_then(|json_string| file.write_all(json_string.as_bytes()))
            .and_then(|_| file.sync_all())
            .unwrap();
        move_file(tmp_scan_state, path.as_ref().join("scan_state")).unwrap();
    }
}

fn move_file<P: AsRef<Path>>(src: P, dst: P) -> Result<(), std::io::Error> {
    if rename(&src, &dst).is_err() {
        copy(&src, &dst)?;
        remove_file(&src)?;
    }
    Ok(())
}
