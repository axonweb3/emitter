use crate::{
    types::{CellType, IndexerTip, Order, RpcSearchKey, Tx},
    Rpc, Submit, SubmitProcess, TipState,
};

use ckb_jsonrpc_types::{CellData, CellInfo, OutPoint};
use ckb_types::{packed, prelude::Unpack};
use std::collections::HashMap;
// H256 + U32
const OUTPOINT_SIZE: usize = 32 + 4;

pub struct CellProcess<T, P, R> {
    key: RpcSearchKey,
    scan_tip: T,
    client: R,
    process_fn: P,
    stop: bool,
}

impl<T, P, R> CellProcess<T, P, R>
where
    T: TipState,
    P: SubmitProcess,
    R: Rpc,
{
    pub fn new(key: RpcSearchKey, tip: T, client: R, process: P) -> Self {
        Self {
            key,
            scan_tip: tip,
            client,
            process_fn: process,
            stop: false,
        }
    }

    pub async fn run(&mut self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(8));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if self.stop || self.process_fn.is_closed() {
                break;
            }
            self.scan(&mut interval).await;
        }
    }

    #[allow(unused_assignments)]
    async fn scan(&mut self, interval: &mut tokio::time::Interval) {
        let indexer_tip = rpc_get!(self.client.get_indexer_tip());
        let old_tip = self.scan_tip.load().clone();

        if indexer_tip.block_number.value().saturating_sub(24) > old_tip.block_number.value() {
            // use tip - 24 as new tip
            let new_tip = {
                let new = rpc_get!(self.client.get_header_by_number(
                    indexer_tip.block_number.value().saturating_sub(24).into(),
                ));
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
                let txs = rpc_get!(self.client.get_transactions(
                    search_key.clone(),
                    Order::Asc,
                    32.into(),
                    cursor.clone()
                ));

                let tx_len = txs.objects.len();
                let mut total_size = 0;
                for tx in txs.objects {
                    match tx {
                        Tx::Grouped(tx_with_cells) => {
                            let tx = rpc_get!(self.client.get_transaction(&tx_with_cells.tx_hash))
                                .unwrap();
                            let header = rpc_get!(self
                                .client
                                .get_header_by_number(tx_with_cells.block_number));
                            let submit_entry =
                                submits.entry(header.hash.clone()).or_insert(Submit {
                                    header,
                                    inputs: Default::default(),
                                    outputs: Default::default(),
                                });
                            for (ty, idx) in tx_with_cells.cells {
                                let index = idx.value() as usize;
                                // header size
                                total_size += 8;
                                match ty {
                                    CellType::Input => {
                                        total_size += OUTPOINT_SIZE;
                                        let outpoint =
                                            tx.inner.inputs[index].previous_output.clone();
                                        submit_entry.inputs.push(outpoint)
                                    }
                                    CellType::Output => {
                                        total_size += OUTPOINT_SIZE;
                                        let cell_info = {
                                            let data = tx.inner.outputs_data.get(index).cloned();
                                            total_size += data
                                                .as_ref()
                                                .map(|a| a.as_bytes().len())
                                                .unwrap_or_default();
                                            let output = tx.inner.outputs[index].clone();
                                            total_size += packed::CellOutput::from(output.clone())
                                                .total_size();
                                            CellInfo {
                                                output,
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

                    if total_size > 1024 * 1024 {
                        let mut cells = submits.drain().map(|(_, v)| v).collect::<Vec<Submit>>();
                        cells.sort_unstable_by_key(|v| v.header.inner.number.value());

                        if !self.process_fn.submit_cells(cells).await {
                            self.stop = true;
                            return;
                        }
                        total_size = 0;
                    }
                }
                total_size = 0;
                if !submits.is_empty() {
                    let mut cells = submits.drain().map(|(_, v)| v).collect::<Vec<Submit>>();
                    cells.sort_unstable_by_key(|v| v.header.inner.number.value());

                    if !self.process_fn.submit_cells(cells).await {
                        self.stop = true;
                        return;
                    }
                }

                if tx_len == 32 {
                    cursor = Some(txs.last_cursor);
                } else {
                    break;
                }
            }
            self.scan_tip.update(new_tip);
        } else {
            interval.tick().await;
        }
    }
}
