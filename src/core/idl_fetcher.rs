use std::{io::Read, sync::Arc};

use anyhow::{Context, Result, anyhow};
use flate2::read::ZlibDecoder;
use log::debug;
use solana_account::Account;
use solana_pubkey::Pubkey;

use crate::core::rpc_provider::{RpcAccountProvider, SolanaRpcProvider};
use crate::utils::progress::Progress;

const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

pub struct IdlFetcher {
    provider: Arc<dyn RpcAccountProvider>,
    progress: Option<Progress>,
}

impl IdlFetcher {
    pub fn new(rpc_url: String, progress: Option<Progress>) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(anyhow!("RPC URL cannot be empty"));
        }
        Ok(Self { provider: Arc::new(SolanaRpcProvider::new(rpc_url)), progress })
    }

    pub fn with_provider(
        provider: Arc<dyn RpcAccountProvider>,
        progress: Option<Progress>,
    ) -> Self {
        Self { provider, progress }
    }

    /// Fetches and parses the Anchor IDL for a given program ID.
    ///
    /// Returns `Ok(Some(json_string))` if the IDL exists and can be parsed,
    /// `Ok(None)` if the IDL account doesn't exist,
    /// or an error if something goes wrong during fetching/parsing.
    pub fn fetch_idl(&self, program_id: &Pubkey) -> Result<Option<String>> {
        self.fetch_idl_with(program_id, |idl_address| {
            let results = self.provider.get_multiple_accounts(&[*idl_address])?;
            results
                .into_iter()
                .next()
                .flatten()
                .ok_or_else(|| anyhow!("AccountNotFound: {}", idl_address))
        })
    }

    /// Fetches and parses Anchor IDLs for multiple program IDs in batch.
    ///
    /// Uses `get_multiple_accounts` to minimize RPC round-trips.
    /// Returns one `(program_id, result)` entry per input, preserving order.
    pub fn fetch_idls(&self, program_ids: &[Pubkey]) -> Vec<(Pubkey, Result<Option<String>>)> {
        self.fetch_idls_with(program_ids, |chunk| self.provider.get_multiple_accounts(chunk))
    }

    fn fetch_idl_with<F>(&self, program_id: &Pubkey, fetch_account: F) -> Result<Option<String>>
    where
        F: FnOnce(&Pubkey) -> Result<Account>,
    {
        let idl_address = get_idl_address(program_id)?;
        debug!("IDL address for program {}: {}", program_id, idl_address);

        let account = match fetch_account(&idl_address) {
            Ok(account) => account,
            Err(e) => {
                if is_account_not_found_error(&e.to_string()) {
                    return Ok(None);
                }
                return Err(anyhow!("Failed to fetch IDL account {}: {}", idl_address, e));
            }
        };

        Ok(Some(parse_idl_account_data(&account.data, program_id)?))
    }

    fn fetch_idls_with<F>(
        &self,
        program_ids: &[Pubkey],
        mut fetch_chunk: F,
    ) -> Vec<(Pubkey, Result<Option<String>>)>
    where
        F: FnMut(&[Pubkey]) -> Result<Vec<Option<Account>>>,
    {
        if program_ids.is_empty() {
            return Vec::new();
        }

        let mut pending = Vec::with_capacity(program_ids.len());
        let mut results: Vec<Option<Result<Option<String>>>> =
            (0..program_ids.len()).map(|_| None).collect();

        for (idx, program_id) in program_ids.iter().enumerate() {
            match get_idl_address(program_id) {
                Ok(idl_address) => pending.push((idx, *program_id, idl_address)),
                Err(e) => results[idx] = Some(Err(e)),
            }
        }

        let total = pending.len();
        let mut requested = 0usize;

        for chunk in pending.chunks(MAX_ACCOUNTS_PER_REQUEST) {
            self.set_progress_message(format!(
                "fetching IDL accounts ({}/{})",
                requested + 1,
                total
            ));

            let idl_addresses: Vec<Pubkey> = chunk.iter().map(|(_, _, idl)| *idl).collect();
            match fetch_chunk(&idl_addresses) {
                Ok(response) => {
                    if response.len() != chunk.len() {
                        for (idx, _, idl_addr) in chunk {
                            results[*idx] = Some(Err(anyhow!(
                                "Failed to fetch IDL account {}: RPC returned count mismatch ({} != {})",
                                idl_addr,
                                response.len(),
                                chunk.len()
                            )));
                        }
                    } else {
                        for ((idx, program_id, _), maybe_account) in
                            chunk.iter().zip(response.into_iter())
                        {
                            let parsed = match maybe_account {
                                Some(account) => {
                                    parse_idl_account_data(&account.data, program_id).map(Some)
                                }
                                None => Ok(None),
                            };
                            results[*idx] = Some(parsed);
                        }
                    }
                }
                Err(e) => {
                    for (idx, _, idl_addr) in chunk {
                        results[*idx] =
                            Some(Err(anyhow!("Failed to fetch IDL account {}: {}", idl_addr, e)));
                    }
                }
            }
            requested += chunk.len();
        }

        let mut ordered_results = Vec::with_capacity(program_ids.len());
        for (idx, program_id) in program_ids.iter().enumerate() {
            let result = results[idx].take().unwrap_or_else(|| {
                Err(anyhow!(
                    "Missing IDL fetch result for program {} (internal state mismatch)",
                    program_id
                ))
            });
            ordered_results.push((*program_id, result));
        }
        ordered_results
    }

    fn set_progress_message(&self, message: impl Into<std::borrow::Cow<'static, str>>) {
        if let Some(progress) = &self.progress {
            progress.set_message(message);
        }
    }
}

fn is_account_not_found_error(error: &str) -> bool {
    error.contains("AccountNotFound") || error.contains("could not find account")
}

/// Computes the Anchor IDL account address for a given program ID.
///
/// The IDL account is derived using:
/// 1. base = PDA with empty seeds from program_id
/// 2. idl_address = create_with_seed(base, "anchor:idl", program_id)
pub fn get_idl_address(program_id: &Pubkey) -> Result<Pubkey> {
    let (base, _) = Pubkey::find_program_address(&[], program_id);
    Pubkey::create_with_seed(&base, "anchor:idl", program_id)
        .map_err(|e| anyhow!("Failed to derive IDL address for {}: {}", program_id, e))
}

/// Parses raw IDL account data (header validation + zlib decompression).
///
/// Account layout:
/// - Bytes 0-7: Discriminator (8 bytes)
/// - Bytes 8-39: Authority pubkey (32 bytes)
/// - Bytes 40-43: Data length (u32 LE)
/// - Bytes 44+: Compressed IDL data (zlib)
fn parse_idl_account_data(data: &[u8], program_id: &Pubkey) -> Result<String> {
    if data.len() < 44 {
        return Err(anyhow!(
            "IDL account data too short: {} bytes (expected at least 44)",
            data.len()
        ));
    }

    let data_len = u32::from_le_bytes([data[40], data[41], data[42], data[43]]) as usize;

    if data.len() < 44 + data_len {
        return Err(anyhow!(
            "IDL account data truncated: has {} bytes, expected {} (header) + {} (data)",
            data.len(),
            44,
            data_len
        ));
    }

    let compressed_data = &data[44..44 + data_len];

    let mut decoder = ZlibDecoder::new(compressed_data);
    let mut decompressed = String::new();
    decoder
        .read_to_string(&mut decompressed)
        .with_context(|| format!("Failed to decompress IDL data for program {}", program_id))?;

    Ok(decompressed)
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Arc;

    use anyhow::anyhow;
    use flate2::Compression;
    use flate2::write::ZlibEncoder;
    use solana_account::Account;
    use solana_pubkey::Pubkey;

    use super::{IdlFetcher, MAX_ACCOUNTS_PER_REQUEST, get_idl_address, parse_idl_account_data};
    use crate::core::rpc_provider::FakeAccountProvider;

    fn dummy_fetcher() -> IdlFetcher {
        IdlFetcher::with_provider(Arc::new(FakeAccountProvider::empty()), None)
    }

    fn build_idl_account(idl_json: &str) -> Account {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(idl_json.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();

        let mut data = Vec::with_capacity(44 + compressed.len());
        data.extend_from_slice(&[0u8; 8]);
        data.extend_from_slice(&[0u8; 32]);
        data.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        data.extend_from_slice(&compressed);

        Account { lamports: 1, data, owner: Pubkey::new_unique(), executable: false, rent_epoch: 0 }
    }

    #[test]
    fn parse_idl_account_data_roundtrip() {
        let program_id = Pubkey::new_unique();
        let expected = r#"{"name":"demo","version":"0.1.0"}"#;
        let account = build_idl_account(expected);
        let parsed = parse_idl_account_data(&account.data, &program_id).unwrap();
        assert_eq!(parsed, expected);
    }

    #[test]
    fn fetch_idl_returns_none_for_account_not_found_error() {
        let fetcher = dummy_fetcher();
        let program_id = Pubkey::new_unique();

        let result = fetcher
            .fetch_idl_with(&program_id, |_| Err(anyhow!("rpc error: AccountNotFound")))
            .unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn fetch_idls_preserves_input_order() {
        let fetcher = dummy_fetcher();
        let first = Pubkey::new_unique();
        let second = Pubkey::new_unique();
        let third = Pubkey::new_unique();
        let program_ids = vec![first, second, third];

        let first_idl = build_idl_account(r#"{"name":"first"}"#);
        let third_idl = build_idl_account(r#"{"name":"third"}"#);
        let first_addr = get_idl_address(&first).unwrap();
        let third_addr = get_idl_address(&third).unwrap();

        let results = fetcher.fetch_idls_with(&program_ids, |chunk| {
            Ok(chunk
                .iter()
                .map(|addr| {
                    if *addr == first_addr {
                        Some(first_idl.clone())
                    } else if *addr == third_addr {
                        Some(third_idl.clone())
                    } else {
                        None
                    }
                })
                .collect())
        });

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, first);
        assert_eq!(results[0].1.as_ref().unwrap().as_deref(), Some(r#"{"name":"first"}"#));
        assert_eq!(results[1].0, second);
        assert!(results[1].1.as_ref().unwrap().is_none());
        assert_eq!(results[2].0, third);
        assert_eq!(results[2].1.as_ref().unwrap().as_deref(), Some(r#"{"name":"third"}"#));
    }

    #[test]
    fn fetch_idls_chunk_error_only_affects_failed_chunk() {
        let fetcher = dummy_fetcher();
        let program_ids: Vec<Pubkey> =
            (0..(MAX_ACCOUNTS_PER_REQUEST + 1)).map(|_| Pubkey::new_unique()).collect();

        let mut call_count = 0usize;
        let results = fetcher.fetch_idls_with(&program_ids, |chunk| {
            call_count += 1;
            if call_count == 1 {
                Ok((0..chunk.len()).map(|_| None).collect())
            } else {
                Err(anyhow!("simulated rpc failure"))
            }
        });

        assert_eq!(results.len(), program_ids.len());
        for (idx, (program_id, result)) in results.iter().enumerate() {
            assert_eq!(*program_id, program_ids[idx]);
            if idx < MAX_ACCOUNTS_PER_REQUEST {
                assert!(result.as_ref().unwrap().is_none());
            } else {
                assert!(result.is_err());
                assert!(
                    result.as_ref().err().unwrap().to_string().contains("simulated rpc failure")
                );
            }
        }
    }
}
