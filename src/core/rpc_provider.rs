// Re-export everything from sonar-sim's rpc_provider so that the CLI
// and sonar-sim share the same RpcAccountProvider trait (critical for
// passing CLI providers to sonar-sim's AccountLoader).
pub use sonar_sim::rpc_provider::*;
