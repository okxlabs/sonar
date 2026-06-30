//! Low-level escape hatches with **no semver guarantees**.
//!
//! Everything needed to drive simulation — the [`Pipeline`](crate::Pipeline)
//! typestate, the loader and fetch seams, the mutation builder, parsing helpers,
//! and the result model — now lives in the crate's stable top-level API. What
//! remains here is the raw JSON-RPC transport, exposed only for callers that need
//! to issue Solana RPC calls the higher-level API doesn't cover (e.g. fetching a
//! historical transaction by signature). These items may change in any release.

// ── Raw JSON-RPC transport ──

pub use crate::rpc_transport::RpcTransport;

pub mod rpc_json {
    pub use crate::rpc_json::*;
}
