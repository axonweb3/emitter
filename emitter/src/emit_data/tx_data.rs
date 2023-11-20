use ckb_jsonrpc_types::{CellInfo, OutPoint, Script, ScriptHashType};
use ethers::abi::AbiEncode;
use ethers::core::types::Bytes;

use emitter_core::types::HeaderViewWithExtension;

use crate::emit_data::{ckb_light_client_abi, image_cell_abi};
use crate::Submit;

pub fn convert_blocks(data: Vec<Submit>) -> Vec<u8> {
    let mut blocks = Vec::new();
    for block in data {
        blocks.push(image_cell_abi::BlockUpdate {
            block_number: block.header.inner.number.into(),
            tx_inputs: convert_inputs(&block.inputs),
            tx_outputs: convert_outputs(&block.outputs),
        });
    }

    image_cell_abi::UpdateCall { blocks }.encode()
}

pub fn convert_headers(headers: Vec<HeaderViewWithExtension>) -> Vec<u8> {
    let mut raw_headers = Vec::with_capacity(headers.len());
    for header in headers {
        let raw = ckb_light_client_abi::Header {
            version: header.inner.inner.version.into(),
            compact_target: header.inner.inner.compact_target.into(),
            timestamp: header.inner.inner.timestamp.into(),
            number: header.inner.inner.number.into(),
            epoch: header.inner.inner.epoch.into(),
            parent_hash: header.inner.inner.parent_hash.to_owned().into(),
            transactions_root: header.inner.inner.transactions_root.to_owned().into(),
            proposals_hash: header.inner.inner.proposals_hash.to_owned().into(),
            extra_hash: header.inner.inner.extra_hash.to_owned().into(),
            dao: header.inner.inner.dao.0,
            nonce: header.inner.inner.nonce.into(),
            block_hash: header.inner.hash.to_owned().into(),
            extension: header.extension.unwrap_or_default().into_bytes().into(),
        };
        raw_headers.push(raw);
    }

    ckb_light_client_abi::UpdateCall {
        headers: raw_headers,
    }
    .encode()
}

fn convert_inputs(inputs: &Vec<OutPoint>) -> Vec<image_cell_abi::OutPoint> {
    let mut res = Vec::new();
    for out_point in inputs {
        res.push(convert_outpoint(out_point));
    }
    res
}

fn convert_outputs(outputs: &Vec<(OutPoint, CellInfo)>) -> Vec<image_cell_abi::CellInfo> {
    let mut res = Vec::new();
    for cell in outputs {
        let type_ = if let Some(ref type_) = cell.1.output.type_ {
            vec![convert_script(type_)]
        } else {
            vec![]
        };

        let data = if let Some(ref data) = cell.1.data {
            data.content.to_owned().into_bytes().into()
        } else {
            Bytes::default()
        };

        res.push(image_cell_abi::CellInfo {
            out_point: convert_outpoint(&cell.0),
            output: image_cell_abi::CellOutput {
                capacity: cell.1.output.capacity.into(),
                lock: convert_script(&cell.1.output.lock),
                type_,
            },
            data,
        });
    }
    res
}

fn convert_outpoint(out_point: &OutPoint) -> image_cell_abi::OutPoint {
    image_cell_abi::OutPoint {
        tx_hash: out_point.tx_hash.to_owned().into(),
        index: out_point.index.into(),
    }
}

fn convert_script(script: &Script) -> image_cell_abi::Script {
    let hash_type = match script.hash_type {
        ScriptHashType::Data => 0,
        ScriptHashType::Type => 1,
        ScriptHashType::Data1 => 2,
    };

    image_cell_abi::Script {
        args: script.args.to_owned().into_bytes().into(),
        code_hash: script.code_hash.to_owned().into(),
        hash_type,
    }
}
