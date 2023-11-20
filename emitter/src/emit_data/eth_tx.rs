use std::convert::TryFrom;

use anyhow::Result;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction::Legacy;
use ethers::types::{Address, TransactionRequest};
use ethers_signers::coins_bip39::English;

pub const IMAGE_CELL_ADDRESS: Address = system_contract_address(0x3);
pub const CKB_LIGHT_CLIENT_ADDRESS: Address = system_contract_address(0x4);

pub async fn send_eth_tx(axon_url: &str, data: Vec<u8>, to: Address) -> Result<()> {
    let provider = Provider::<Http>::try_from(axon_url)?;
    let wallet = MnemonicBuilder::<English>::default()
        .phrase("test test test test test test test test test test test junk")
        .build()
        .unwrap();

    let from: Address = wallet.address();
    let nonce = provider.get_transaction_count(from, None).await?;

    let transaction_request = TransactionRequest::new()
        .chain_id(0x41786f6e)
        .to(to)
        .data(data)
        .from(from)
        .gas_price(1)
        .gas(21000)
        .nonce(nonce);

    let tx = Legacy(transaction_request);
    let signature: Signature = wallet.sign_transaction(&tx).await?;

    provider
        .send_raw_transaction(tx.rlp_signed(&signature))
        .await?
        .await?
        .expect("failed to send eth tx");

    Ok(())
}

const fn system_contract_address(addr: u8) -> H160 {
    H160([
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, addr,
    ])
}
