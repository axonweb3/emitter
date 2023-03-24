use ckb_jsonrpc_types::{CellInfo, OutPoint, Script, ScriptHashType};
use ethers::abi::AbiEncode;
use ethers::core::types::Bytes;

use crate::emit_data::image_cell_abi;
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

// header:  convert_header(&data.header),
// fn convert_header(header: &HeaderView) -> image_cell_abi::Header {
//     image_cell_abi::Header {
//         version:           header.inner.version.into(),
//         compact_target:    header.inner.compact_target.into(),
//         timestamp:         header.inner.timestamp.into(),
//         number:            header.inner.number.into(),
//         epoch:             header.inner.epoch.into(),
//         parent_hash:       header.inner.parent_hash.to_owned().into(),
//         transactions_root: header.inner.transactions_root.to_owned().into(),
//         proposals_hash:    header.inner.proposals_hash.to_owned().into(),
//         uncles_hash:       header.inner.extra_hash.to_owned().into(),
//         dao:               header.inner.dao.0,
//         nonce:             header.inner.nonce.into(),
//         block_hash:        header.hash.to_owned().into(),
//     }
// }

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
