# emitter

## Usage

Build binary from source

```bash
cargo build --release
```

Connect to default ckb rpc service at http://127.0.0.1:8114 and stores the indexer data at /tmp/emitter folder

```bash
RUST_LOG=info ./target/release/emitter -s /tmp/emitter
```

Run `emitter --help` for more information

## Websocket Subscription

**This module is mutually exclusive with http rpc**

```bash
RUST_LOG=info ./target/release/emitter --ws
```

### header_sync

```js
let socket = new WebSocket("ws://localhost:8120")

socket.onmessage = function(event) {
  console.log(`Data received from server: ${event.data}`);
}

socket.send(`{"id": 2, "jsonrpc": "2.0", "method": "emitter_subscription", "params": ["header_sync", "0x0"]}`)

socket.send(`{"id": 2, "jsonrpc": "2.0", "method": "emitter_unsubscribe", "params": [0]}`)
```

#### Parameters

u64, start block number

#### Return

list of [HeaderView](https://github.com/nervosnetwork/ckb/tree/develop/rpc#type-headerview)

### cell_filter

```js
let socket = new WebSocket("ws://localhost:8120")

socket.onmessage = function(event) {
    console.log(`Data received from server: $ {
        event.data
    }`);
}

socket.send(` {
    "id": 2,
    "jsonrpc": "2.0",
    "method": "emitter_subscription",
    "params": ["cell_filter", {
        "script": {
            "code_hash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            "hash_type": "type",
            "args": "0x5989ae415bb667931a99896e5fbbfad9ba53a223"
        },
        "script_type": "lock"
    },
    "0x0"]
}`)

socket.send(` {
    "id": 2,
    "jsonrpc": "2.0",
    "method": "emitter_unsubscribe",
    "params": [0]
}`)
```

#### Parameters

```
search_key:
    script - Script
    script_type - enum, lock | type
    script_search_mode - enum, prefix | exact | null - Script search mode, optional default is `prefix`, means search script with prefix
    filter - filter cells by following conditions, all conditions are optional
        script: if search script type is lock, filter cells by type script prefix, and vice versa
        script_len_range: [u64; 2], filter cells by script len range, [inclusive, exclusive]
        output_data_len_range: [u64; 2], filter cells by output data len range, [inclusive, exclusive]
        output_capacity_range: [u64; 2], filter cells by output capacity range, [inclusive, exclusive]
start: u64, start block number
```

#### Return

```
{
    [   
        {
            "header": HeaderView, 
            "inputs": [
                OutPoint
            ], 
            "outputs": [
                [
                    OutPoint, CellInfo
                ]
            ]
        }
    ]
}
```
- [HeaderView](https://github.com/nervosnetwork/ckb/tree/develop/rpc#type-headerview)
- [OutPoint](https://github.com/nervosnetwork/ckb/tree/develop/rpc#type-outpoint)
- [CellInfo](https://github.com/nervosnetwork/ckb/tree/develop/rpc#type-cellinfo)

## RPC

### register

Register the cell you want to track

#### Parameters

```
search_key:
    script - Script
    script_type - enum, lock | type
    script_search_mode - enum, prefix | exact | null - Script search mode, optional default is `prefix`, means search script with prefix
    filter - filter cells by following conditions, all conditions are optional
        script: if search script type is lock, filter cells by type script prefix, and vice versa
        script_len_range: [u64; 2], filter cells by script len range, [inclusive, exclusive]
        output_data_len_range: [u64; 2], filter cells by output data len range, [inclusive, exclusive]
        output_capacity_range: [u64; 2], filter cells by output capacity range, [inclusive, exclusive]
start: u64, start block number
```

#### Returns

```
bool, registration success or failure
```

#### Examples

```bash
echo '{
    "id": 2,
    "jsonrpc": "2.0",
    "method": "register",
    "params": [
        {
            "script": {
                "code_hash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
                "hash_type": "type",
                "args": "0x5989ae415bb667931a99896e5fbbfad9ba53a223"
            },
            "script_type": "lock"
        },
        "0x0"
    ]
}' \
| curl -H 'content-type: application/json' -d @- \
http://localhost:8120
```

<details>
    <summary>click to expand result</summary>
<p>

```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 2
}
```

</p>
</details>


### delete

Delete the registered cell

#### Parameters

```
search_key:
    script - Script, supports prefix search
    script_type - enum, lock | type
    filter - filter cells by following conditions, all conditions are optional
        script: if search script type is lock, filter cells by type script prefix, and vice versa
        script_len_range: [u64; 2], filter cells by script len range, [inclusive, exclusive]
        output_data_len_range: [u64; 2], filter cells by output data len range, [inclusive, exclusive]
        output_capacity_range: [u64; 2], filter cells by output capacity range, [inclusive, exclusive]
```

#### Returns

```
bool, delete success or failure
```

#### Examples

```bash
echo '{
    "id": 2,
    "jsonrpc": "2.0",
    "method": "delete",
    "params": [
        {
            "script": {
                "code_hash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
                "hash_type": "type",
                "args": "0x5989ae415bb667931a99896e5fbbfad9ba53a223"
            },
            "script_type": "lock"
        }
    ]
}' \
| curl -H 'content-type: application/json' -d @- \
http://localhost:8120
```

<details>
    <summary>click to expand result</summary>
<p>

```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 2
}
```

</p>
</details>

### header_sync_start

Set the header from which to start synchronization, if not set, start with genesis block

#### Parameters

```
u64, start block number
```

#### Returns

```
bool, set success or failure
```

#### Examples

```bash
echo '{
    "id": 2,
    "jsonrpc": "2.0",
    "method": "header_sync_start",
    "params": ["0x2f00"]
}' \
| curl -H 'content-type: application/json' -d @- \
http://localhost:8120
```

<details>
    <summary>click to expand result</summary>
<p>

```json
{
  "jsonrpc": "2.0",
  "result": true,
  "id": 2
}
```

</p>
</details>

### info

Returns the state of the cell being tracked


#### Parameters

```
null
```

#### Returns

```
objects:
    cells - tracing cells collection
        search_key:
        state
            block_number: scan tip block number
            block_hash: scan tip block hash
```


#### Examples

```bash
echo '{
    "id": 2,
    "jsonrpc": "2.0",
    "method": "info",
    "params": []
}' \
| curl -H 'content-type: application/json' -d @- \
http://localhost:8120
```

<details>
    <summary>click to expand result</summary>
<p>

```json
{
  "jsonrpc": "2.0",
  "result": [
    "cell_states": [
        {
            "script": {
                    "code_hash": "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
                    "hash_type": "type",
                    "args": "0x5989ae415bb667931a99896e5fbbfad9ba53a223"
                },
            "script_type": "lock",
            "filter": null
        },
        {
            "block_hash": "0x9bfe99915bd967629d2bccd785ae2a972d2ec82cb8e0d4ebc86baa5c14d89f85",
            "block_number": "0x86f6cd"
        }
    ],
    "header_state":{
      "block_hash":"0x9e2f631a52404a973b94e72f906e489ce840a321789bd00286b549bd01737133",
      "block_number":"0xf00"
   }
  ],
  "id": 1
}

```

</p>
</details>
