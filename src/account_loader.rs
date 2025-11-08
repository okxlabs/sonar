use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use anyhow::{anyhow, Context, Result};
use log::{debug, trace, warn};
use solana_account::{Account, ReadableAccount};
use solana_address_lookup_table_interface::state::AddressLookupTable;
use solana_client::rpc_client::RpcClient;
use solana_clock::Slot;
use solana_pubkey::Pubkey as LitePubkey;
use solana_sdk::{
    account::Account as LegacyAccount,
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    clock::Clock,
    pubkey::Pubkey,
    slot_hashes::SlotHashes,
    sysvar::SysvarId,
    transaction::VersionedTransaction,
};
use std::sync::Mutex;

use crate::{
    cli::ProgramReplacement,
    transaction::{collect_account_plan, AddressLookupPlan},
};

const MAX_ACCOUNTS_PER_REQUEST: usize = 100;

pub struct AccountLoader {
    client: Arc<RpcClient>,
    cache: Mutex<HashMap<Pubkey, Account>>,
}

impl AccountLoader {
    pub fn new(rpc_url: String) -> Result<Self> {
        if rpc_url.is_empty() {
            return Err(anyhow!("RPC URL 不能为空"));
        }
        Ok(Self {
            client: Arc::new(RpcClient::new(rpc_url)),
            cache: Mutex::new(HashMap::new()),
        })
    }

    pub fn load_for_transaction(
        &self,
        tx: &VersionedTransaction,
        _replacements: &[ProgramReplacement],
    ) -> Result<ResolvedAccounts> {
        let plan = collect_account_plan(tx);
        let mut accounts = HashMap::new();

        // 预先加载静态账户
        self.fetch_accounts(&plan.static_accounts, &mut accounts)
            .context("拉取静态账户信息失败")?;

        self.fetch_accounts(&[Clock::id(), SlotHashes::id()], &mut accounts)
            .context("拉取系统变量账户失败")?;

        self.ensure_upgradeable_dependencies(&mut accounts)
            .context("加载可升级程序元数据失败")?;

        let mut lookups = Vec::new();
        let mut writable_lookup_accounts = Vec::new();
        let mut readonly_lookup_accounts = Vec::new();

        // 处理地址查找表
        // 与 `build_lookup_locations` 保持一致的顺序：先聚合所有可写索引，再聚合所有只读索引
        for lookup_plan in &plan.address_lookups {
            let resolved = self.load_lookup_table(lookup_plan, &mut accounts)?;
            writable_lookup_accounts.extend(resolved.writable_addresses.iter().copied());
            readonly_lookup_accounts.extend(resolved.readonly_addresses.iter().copied());
            lookups.push(resolved);
        }

        if !writable_lookup_accounts.is_empty() {
            self.fetch_accounts(&writable_lookup_accounts, &mut accounts)
                .with_context(|| {
                    format!(
                        "加载地址查找表可写账户失败: [{}]",
                        format_pubkeys(&writable_lookup_accounts)
                    )
                })?;
            self.ensure_upgradeable_dependencies(&mut accounts)
                .context("处理地址查找表可写账户时加载可升级程序依赖失败")?;
        }

        if !readonly_lookup_accounts.is_empty() {
            self.fetch_accounts(&readonly_lookup_accounts, &mut accounts)
                .with_context(|| {
                    format!(
                        "加载地址查找表只读账户失败: [{}]",
                        format_pubkeys(&readonly_lookup_accounts)
                    )
                })?;
            self.ensure_upgradeable_dependencies(&mut accounts)
                .context("处理地址查找表只读账户时加载可升级程序依赖失败")?;
        }

        Ok(ResolvedAccounts { accounts, lookups })
    }

    fn ensure_upgradeable_dependencies(
        &self,
        accounts: &mut HashMap<Pubkey, Account>,
    ) -> Result<()> {
        loop {
            let mut missing = Vec::new();
            for account in accounts.values() {
                if sdk_pubkey_from_lite(&account.owner) != bpf_loader_upgradeable::id() {
                    continue;
                }
                if let Ok(state) =
                    bincode::deserialize::<UpgradeableLoaderState>(account.data.as_slice())
                {
                    if let UpgradeableLoaderState::Program {
                        programdata_address,
                    } = state
                    {
                        if !accounts.contains_key(&programdata_address) {
                            missing.push(programdata_address);
                        }
                    }
                }
            }

            if missing.is_empty() {
                break;
            }

            self.fetch_accounts(&missing, accounts).with_context(|| {
                format!("拉取 ProgramData 账户失败: [{}]", format_pubkeys(&missing))
            })?;
        }

        Ok(())
    }

    fn load_lookup_table(
        &self,
        plan: &AddressLookupPlan,
        accounts: &mut HashMap<Pubkey, Account>,
    ) -> Result<ResolvedLookup> {
        // 获取查找表账户
        self.fetch_accounts(&[plan.account_key], accounts)
            .with_context(|| format!("获取地址查找表账户 `{}` 失败", plan.account_key))?;

        let table_account = accounts
            .get(&plan.account_key)
            .ok_or_else(|| anyhow!("缓存中缺少地址查找表账户 `{}`", plan.account_key))?;

        let lookup_table = AddressLookupTable::deserialize(table_account.data())
            .map_err(|err| anyhow!("解析地址查找表 `{}` 失败: {err}", plan.account_key))?;
        let meta = lookup_table.meta.clone();
        let all_addresses = lookup_table.addresses.to_vec();

        let writable_addresses = resolve_lookup_indexes(&all_addresses, &plan.writable_indexes)
            .with_context(|| format!("解析地址查找表 `{}` 的可写索引失败", plan.account_key))?;
        let readonly_addresses = resolve_lookup_indexes(&all_addresses, &plan.readonly_indexes)
            .with_context(|| format!("解析地址查找表 `{}` 的只读索引失败", plan.account_key))?;

        Ok(ResolvedLookup {
            account_key: plan.account_key,
            writable_indexes: plan.writable_indexes.clone(),
            readonly_indexes: plan.readonly_indexes.clone(),
            writable_addresses,
            readonly_addresses,
            last_extended_slot: meta.last_extended_slot,
            deactivation_slot: meta.deactivation_slot,
        })
    }

    fn fetch_accounts(
        &self,
        pubkeys: &[Pubkey],
        destination: &mut HashMap<Pubkey, Account>,
    ) -> Result<()> {
        let mut unique = Vec::new();
        let mut seen = HashSet::new();
        for key in pubkeys {
            if destination.contains_key(key) {
                continue;
            }
            if seen.insert(*key) {
                unique.push(*key);
            }
        }

        if unique.is_empty() {
            return Ok(());
        }

        trace!(
            "准备拉取 {} 个账户: [{}]",
            unique.len(),
            format_pubkeys(&unique)
        );

        let mut to_fetch = Vec::new();
        {
            let cache = self.cache.lock().unwrap();
            for key in unique {
                if let Some(account) = cache.get(&key) {
                    destination.insert(key, account.clone());
                } else {
                    to_fetch.push(key);
                }
            }
        }

        if to_fetch.is_empty() {
            return Ok(());
        }

        for chunk in to_fetch.chunks(MAX_ACCOUNTS_PER_REQUEST) {
            let response = self.client.get_multiple_accounts(chunk).with_context(|| {
                format!(
                    "调用 getMultipleAccounts 失败，账户列表: [{}]",
                    format_pubkeys(chunk)
                )
            })?;

            if response.len() != chunk.len() {
                return Err(anyhow!(
                    "RPC 返回数量与请求不匹配 ({} != {})",
                    response.len(),
                    chunk.len()
                ));
            }

            for (pubkey, maybe_account) in chunk.iter().zip(response.into_iter()) {
                let account = match maybe_account {
                    Some(legacy) => convert_account(legacy),
                    None => match self.client.get_account(pubkey) {
                        Ok(single) => convert_account(single),
                        Err(err) => {
                            warn!("单账户拉取失败 `{pubkey}`: {err}; 使用占位账户");
                            synthesize_missing_account(pubkey)
                        }
                    },
                };

                destination.insert(*pubkey, account.clone());

                let mut cache = self.cache.lock().unwrap();
                cache.insert(*pubkey, account);
            }
        }

        debug!("成功拉取账户: [{}]", format_pubkeys(pubkeys));
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedAccounts {
    pub accounts: HashMap<Pubkey, Account>,
    pub lookups: Vec<ResolvedLookup>,
}

#[derive(Debug, Clone)]
pub struct ResolvedLookup {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
    pub writable_addresses: Vec<Pubkey>,
    pub readonly_addresses: Vec<Pubkey>,
    pub last_extended_slot: Slot,
    pub deactivation_slot: Slot,
}

fn resolve_lookup_indexes(addresses: &[Pubkey], indexes: &[u8]) -> Result<Vec<Pubkey>> {
    indexes
        .iter()
        .map(|idx| {
            addresses
                .get(*idx as usize)
                .copied()
                .ok_or_else(|| anyhow!("索引 {idx} 超出地址查找表范围"))
        })
        .collect()
}

fn format_pubkeys(pubkeys: &[Pubkey]) -> String {
    const MAX_DISPLAY: usize = 10;
    if pubkeys.len() <= MAX_DISPLAY {
        return pubkeys
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
    }
    let mut rendered = pubkeys
        .iter()
        .take(MAX_DISPLAY)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    rendered.push(format!("... 共 {} 个", pubkeys.len()));
    rendered.join(", ")
}

fn convert_account(account: LegacyAccount) -> Account {
    Account {
        lamports: account.lamports,
        data: account.data,
        owner: LitePubkey::from(account.owner.to_bytes()),
        executable: account.executable,
        rent_epoch: account.rent_epoch,
    }
}

fn sdk_pubkey_from_lite(pubkey: &LitePubkey) -> Pubkey {
    Pubkey::new_from_array(pubkey.to_bytes())
}

fn synthesize_missing_account(pubkey: &Pubkey) -> Account {
    warn!("链上未找到账户 `{pubkey}`，使用空白占位账户继续模拟");
    Account {
        lamports: 0,
        data: Vec::new(),
        owner: LitePubkey::default(),
        executable: false,
        rent_epoch: 0,
    }
}
