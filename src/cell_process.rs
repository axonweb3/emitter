use ckb_jsonrpc_types::{CellData, CellInfo, OutPoint};
use ckb_types::{packed, prelude::Unpack};
use std::{collections::HashMap, sync::atomic::Ordering};

use crate::{
    rpc_client::{CellType, IndexerTip, Order, RpcClient, Tx},
    rpc_server::RpcSearchKey,
    submit_to_realyer, ScanTip, Submit,
};
pub(crate) struct CellProcess {
    pub key: RpcSearchKey,
    pub scan_tip: ScanTip,
    pub client: RpcClient,
}

impl CellProcess {
    pub async fn run(&mut self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(8));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            self.scan().await;
        }
    }

    async fn scan(&self) {
        let indexer_tip = self.client.get_indexer_tip().await.unwrap();
        let old_tip = unsafe { &*self.scan_tip.0 .0.load(Ordering::Acquire) }.clone();

        if indexer_tip.block_number.value().saturating_sub(24) > old_tip.block_number.value() {
            // use tip - 24 as new tip
            let new_tip = {
                let new = self
                    .client
                    .get_header_by_number(
                        indexer_tip.block_number.value().saturating_sub(24).into(),
                    )
                    .await
                    .unwrap();
                IndexerTip {
                    block_hash: new.hash,
                    block_number: new.inner.number,
                }
            };

            let search_key = self
                .key
                .clone()
                .into_key(Some([old_tip.block_number, new_tip.block_number]));

            let mut cursor = None;

            let mut submits = HashMap::new();

            loop {
                let txs = self
                    .client
                    .get_transactions(search_key.clone(), Order::Asc, 128.into(), cursor)
                    .await
                    .unwrap();

                let tx_len = txs.objects.len();

                for tx in txs.objects {
                    match tx {
                        Tx::Grouped(tx_with_cells) => {
                            let tx = self
                                .client
                                .get_transaction(&tx_with_cells.tx_hash)
                                .await
                                .unwrap()
                                .unwrap();
                            let header = self
                                .client
                                .get_header_by_number(tx_with_cells.block_number)
                                .await
                                .unwrap();
                            let submit_entry =
                                submits.entry(header.hash.clone()).or_insert(Submit {
                                    header,
                                    inputs: Default::default(),
                                    outputs: Default::default(),
                                });
                            for (ty, idx) in tx_with_cells.cells {
                                let index = idx.value() as usize;
                                match ty {
                                    CellType::Input => {
                                        let outpoint = OutPoint {
                                            tx_hash: tx_with_cells.tx_hash.clone(),
                                            index: idx,
                                        };
                                        submit_entry.inputs.push(outpoint)
                                    }
                                    CellType::Output => {
                                        let cell_info = {
                                            let data = tx.inner.outputs_data.get(index).cloned();
                                            CellInfo {
                                                output: tx.inner.outputs[index].clone(),
                                                data: data.map(|d| CellData {
                                                    hash: packed::CellOutput::calc_data_hash(
                                                        d.as_bytes(),
                                                    )
                                                    .unpack(),
                                                    content: d,
                                                }),
                                            }
                                        };
                                        let outpoint = OutPoint {
                                            tx_hash: tx_with_cells.tx_hash.clone(),
                                            index: idx,
                                        };
                                        submit_entry.outputs.push((outpoint, cell_info));
                                    }
                                }
                            }
                        }
                        Tx::Ungrouped(_) => unreachable!(),
                    }
                }

                if tx_len == 128 {
                    cursor = Some(txs.last_cursor);
                } else {
                    break;
                }
            }

            submit_to_realyer(submits).await;
            let raw = self
                .scan_tip
                .0
                 .0
                .swap(Box::into_raw(Box::new(new_tip)), Ordering::AcqRel);

            unsafe {
                drop(Box::from_raw(raw));
            }
        }
    }
}
