use ckb_jsonrpc_types::HeaderView;
use serde::{ser::SerializeStruct, Deserialize, Deserializer, Serialize, Serializer};
use std::{
    fs::{copy, create_dir_all, remove_file, rename, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::AtomicPtr, Arc},
};

use crate::{
    cell_process::{CellProcess, RpcSubmit},
    header_sync::HeaderSyncProcess,
    rpc_client::{IndexerTip, RpcClient},
    rpc_server::RpcSearchKey,
    ScanTip, ScanTipInner,
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
    cell_handles: Arc<dashmap::DashMap<RpcSearchKey, tokio::task::JoinHandle<()>>>,
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

        Self {
            cell_handles: Arc::new(dashmap::DashMap::with_capacity(state.cell_states.len())),
            state,
            path,
        }
    }

    pub async fn run(&mut self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            // clean shutdown task
            let mut shutdown_task = Vec::new();
            self.cell_handles.retain(|k, v| {
                if v.is_finished() {
                    shutdown_task.push(k.clone());
                    false
                } else {
                    true
                }
            });
            shutdown_task.into_iter().for_each(|k| {
                self.state.cell_states.remove(&k);
            });

            self.dump_to_dir(self.path.clone());
        }
    }

    pub fn spawn_cells(
        &self,
        client: RpcClient,
    ) -> Arc<dashmap::DashMap<RpcSearchKey, tokio::task::JoinHandle<()>>> {
        if !self.state.cell_states.is_empty() {
            for kv in self.state.cell_states.iter() {
                let mut cell_process = CellProcess {
                    key: kv.key().clone(),
                    scan_tip: kv.value().clone(),
                    client: client.clone(),
                    process_fn: RpcSubmit,
                    stop: false,
                };

                let handle = tokio::spawn(async move {
                    cell_process.run().await;
                });
                self.cell_handles.insert(kv.key().clone(), handle);
            }
        }
        self.cell_handles.clone()
    }

    pub fn spawn_header_sync(&self, client: RpcClient) {
        let state = self.state.header_state.clone();

        let mut header_sync = HeaderSyncProcess {
            scan_tip: state,
            client,
            process_fn: RpcSubmit,
            stop: false,
        };
        tokio::spawn(async move {
            header_sync.run().await;
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
