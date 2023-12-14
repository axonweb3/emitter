use std::convert::TryFrom;

use anyhow::Result;
use ethers::core::k256::ecdsa::SigningKey;
use ethers::prelude::*;
use ethers::types::transaction::eip2718::TypedTransaction::Legacy;
use ethers::types::{Address, TransactionRequest};
use ethers_signers::coins_bip39::English;

pub const IMAGE_CELL_ADDRESS: Address = system_contract_address(0x3);
pub const CKB_LIGHT_CLIENT_ADDRESS: Address = system_contract_address(0x2);

pub async fn send_eth_tx(axon_url: &str, data: Vec<u8>, to: Address) -> Result<()> {
    let provider = Provider::<Http>::try_from(axon_url)?;
    let wallet = wallet(None);

    let from: Address = wallet.address();
    let nonce = provider.get_transaction_count(from, None).await?;
    let chain_id = provider.get_chainid().await?;

    let transaction_request = TransactionRequest::new()
        .chain_id(chain_id.as_usize())
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

pub fn wallet(private_key: Option<&[u8]>) -> &'static Wallet<SigningKey> {
    static WALLET: std::sync::OnceLock<Wallet<SigningKey>> = std::sync::OnceLock::new();
    WALLET.get_or_init(|| match private_key {
        Some(s) => LocalWallet::from_bytes(s).expect("failed to create wallet"),
        None => MnemonicBuilder::<English>::default()
            .phrase("test test test test test test test test test test test junk")
            .build()
            .unwrap(),
    })
}

const fn system_contract_address(addr: u8) -> H160 {
    H160([
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0xff, addr,
    ])
}
