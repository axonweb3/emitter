use std::{
    fs::{copy, create_dir_all, remove_file, rename, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{cell_process::CellProcess, rpc_client::RpcClient, rpc_server::RpcSearchKey, ScanTip};

pub(crate) struct GlobalState {
    pub state: Arc<dashmap::DashMap<RpcSearchKey, ScanTip>>,
    path: PathBuf,
}

impl Drop for GlobalState {
    fn drop(&mut self) {
        self.dump_to_dir(self.path.clone())
    }
}

impl GlobalState {
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
        let res = dashmap::DashMap::with_capacity(self.state.len());
        if !self.state.is_empty() {
            for kv in self.state.iter() {
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

    pub fn load_from_dir(path: PathBuf) -> Self {
        let db_path = path.join("scan_state");

        let state = match File::open(&db_path) {
            Ok(f) => {
                let cells: Vec<(RpcSearchKey, ScanTip)> =
                    serde_json::from_reader(f).unwrap_or_default();

                Arc::new(cells.into_iter().collect())
            }
            Err(e) => {
                log::warn!(
                    "Failed to open state db, file: {:?}, error: {:?}",
                    db_path,
                    e
                );
                Default::default()
            }
        };

        GlobalState { state, path }
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
        let cell_state_iter = self
            .state
            .iter()
            .map(|kv| (kv.key().clone(), kv.value().clone()))
            .collect::<Vec<_>>();
        // empty file and dump the json string to it
        file.set_len(0)
            .and_then(|_| serde_json::to_string(&cell_state_iter).map_err(Into::into))
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
