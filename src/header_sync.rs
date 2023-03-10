use crate::{
    rpc_client::{IndexerTip, RpcClient},
    SubmitProcess, TipState,
};

pub(crate) struct HeaderSyncProcess<T, P> {
    pub scan_tip: T,
    pub client: RpcClient,
    pub process_fn: P,
    pub stop: bool,
}

impl<T, P> HeaderSyncProcess<T, P>
where
    T: TipState,
    P: SubmitProcess,
{
    pub(crate) async fn run(&mut self) {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(8));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if self.stop || self.process_fn.is_closed() {
                break;
            }
            self.scan(&mut interval).await;
        }
    }

    async fn scan(&mut self, interval: &mut tokio::time::Interval) {
        let indexer_tip = self.client.get_indexer_tip().await.unwrap();
        let old_tip = self.scan_tip.load().clone();

        if indexer_tip.block_number.value().saturating_sub(24) > old_tip.block_number.value() {
            let new_tip = {
                let new = self
                    .client
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

            for i in old_tip.block_number.value()..new_tip.block_number.value() {
                let header = self.client.get_header_by_number(i.into()).await.unwrap();
                headers.push(header);
            }

            if !self.process_fn.submit_headers(headers).await {
                self.stop = true
            }

            self.scan_tip.update(new_tip);
        } else {
            interval.tick().await;
        }
    }
}
