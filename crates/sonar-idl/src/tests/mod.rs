use crate::indexed::IndexedIdl;

fn hello_anchor_indexed_idl() -> IndexedIdl {
    serde_json::from_str(
        r#"{
            "address": "BYFW1vhC1ohxwRbYoLbAWs86STa25i9sD5uEusVjTYNd",
            "metadata": { "name": "hello_anchor", "version": "0.1.0", "spec": "0.1.0" },
            "instructions": [{
                "name": "initialize",
                "discriminator": [175, 175, 109, 31, 13, 152, 155, 237],
                "accounts": [
                    { "name": "new_account", "writable": true, "signer": true },
                    { "name": "signer", "writable": true, "signer": true },
                    { "name": "system_program", "address": "11111111111111111111111111111111" }
                ],
                "args": [{ "name": "data", "type": "u64" }]
            }],
            "types": [{
                "name": "NewAccount",
                "type": { "kind": "struct", "fields": [{ "name": "data", "type": "u64" }] }
            }]
        }"#,
    )
    .unwrap()
}

mod account;
mod decode;
mod event;
mod idl;
mod instruction;
