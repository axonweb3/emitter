use std::convert::TryFrom;
use std::str::FromStr;

use anyhow::Result;
use ethers::prelude::*;
use ethers::signers::{LocalWallet, Signer};
use ethers::types::transaction::eip2718::TypedTransaction::Legacy;
use ethers::types::{Address, TransactionRequest};

const ADDRESS: &str = "0x8ab0CF264DF99D83525e9E11c7e4db01558AE1b1";
const PRIVATE_KEY: &str = "37aa0f893d05914a4def0460c0a984d3611546cfb26924d7a7ca6e0db9950a2d";

pub const IMAGE_CELL_ADDRESS: Address = system_contract_address(0x3);
pub const _CKB_LIGHT_CLIENT_ADDRESS: Address = system_contract_address(0x4);

pub async fn send_eth_tx(axon_url: &str, data: Vec<u8>, to: Address) -> Result<()> {
    let provider = Provider::<Http>::try_from(axon_url)?;

    let from: Address = ADDRESS.parse().unwrap();
    let nonce = provider.get_transaction_count(from, None).await?;

    let transaction_request = TransactionRequest::new()
        .chain_id(2022)
        .to(to)
        .data(data)
        .from(from)
        .gas_price(1)
        .gas(21000)
        .nonce(nonce);

    let wallet = LocalWallet::from_str(PRIVATE_KEY).expect("failed to create wallet");
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
