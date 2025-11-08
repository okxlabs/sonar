use std::path::Path;

use anyhow::{anyhow, Context, Result};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use bs58::decode::Error as Base58Error;
use serde::Serialize;
use solana_sdk::{
    message::VersionedMessage,
    pubkey::Pubkey,
    transaction::{TransactionVersion, VersionedTransaction},
};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RawTransactionEncoding {
    Base58,
    Base64,
}

#[derive(Debug, Clone)]
pub struct ParsedTransaction {
    pub encoding: RawTransactionEncoding,
    pub version: TransactionVersion,
    pub transaction: VersionedTransaction,
    pub summary: TransactionSummary,
}

#[derive(Debug, Clone)]
pub struct MessageAccountPlan {
    pub static_accounts: Vec<Pubkey>,
    pub address_lookups: Vec<AddressLookupPlan>,
}

impl MessageAccountPlan {
    pub fn from_transaction(tx: &VersionedTransaction) -> Self {
        let static_accounts = tx.message.static_account_keys().to_vec();
        let address_lookups = build_address_lookup_plan(&tx.message);
        Self {
            static_accounts,
            address_lookups,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AddressLookupPlan {
    pub account_key: Pubkey,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransactionSummary {
    pub signatures: Vec<String>,
    pub recent_blockhash: String,
    pub static_accounts: Vec<AccountKeySummary>,
    pub instructions: Vec<InstructionSummary>,
    pub address_table_lookups: Vec<AddressLookupSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountKeySummary {
    pub index: usize,
    pub pubkey: String,
    pub signer: bool,
    pub writable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstructionSummary {
    pub index: usize,
    pub program: AccountReferenceSummary,
    pub accounts: Vec<AccountReferenceSummary>,
    pub data_length: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum AccountSourceSummary {
    Static,
    Lookup {
        table_account: String,
        lookup_index: u8,
        writable: bool,
    },
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountReferenceSummary {
    pub index: usize,
    pub pubkey: Option<String>,
    pub signer: bool,
    pub writable: bool,
    pub source: AccountSourceSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddressLookupSummary {
    pub account_key: String,
    pub writable_indexes: Vec<u8>,
    pub readonly_indexes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct LookupLocation {
    table_account: Pubkey,
    table_index: u8,
    writable: bool,
}

pub fn read_raw_transaction(inline: Option<String>, tx_file: Option<&Path>) -> Result<String> {
    match (inline, tx_file) {
        (Some(tx), None) => {
            let trimmed = tx.trim();
            if trimmed.is_empty() {
                Err(anyhow!("原始交易字符串不能为空"))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        (None, Some(path)) => {
            let content = std::fs::read_to_string(path)
                .with_context(|| format!("读取交易文件失败: {}", path.display()))?;
            let trimmed = content.trim();
            if trimmed.is_empty() {
                Err(anyhow!(
                    "文件 `{}` 不包含有效的原始交易内容",
                    path.display()
                ))
            } else {
                Ok(trimmed.to_owned())
            }
        }
        (Some(_), Some(_)) => Err(anyhow!("请仅指定 --tx 或 --tx-file 中的一个参数")),
        (None, None) => Err(anyhow!("未提供原始交易，请使用 --tx 或 --tx-file")),
    }
}

pub fn parse_raw_transaction(raw: &str) -> Result<ParsedTransaction> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("原始交易字符串为空"));
    }

    let mut errors = Vec::new();

    for encoding in [
        RawTransactionEncoding::Base64,
        RawTransactionEncoding::Base58,
    ] {
        match decode_bytes(trimmed, encoding) {
            Ok(bytes) => match bincode::deserialize::<VersionedTransaction>(&bytes) {
                Ok(transaction) => {
                    let version = transaction.version();
                    let account_plan = MessageAccountPlan::from_transaction(&transaction);
                    let summary = TransactionSummary::from_transaction(&transaction, &account_plan);
                    return Ok(ParsedTransaction {
                        encoding,
                        version,
                        transaction,
                        summary,
                    });
                }
                Err(err) => errors.push(anyhow!(
                    "{} 反序列化失败: {err}",
                    match encoding {
                        RawTransactionEncoding::Base58 => "Base58",
                        RawTransactionEncoding::Base64 => "Base64",
                    }
                )),
            },
            Err(err) => errors.push(err),
        }
    }

    let merged = errors
        .into_iter()
        .map(|err| err.to_string())
        .collect::<Vec<_>>()
        .join("； ");
    Err(anyhow!("无法解析原始交易: {merged}"))
}

pub fn collect_account_plan(tx: &VersionedTransaction) -> MessageAccountPlan {
    MessageAccountPlan::from_transaction(tx)
}

fn decode_bytes(input: &str, encoding: RawTransactionEncoding) -> Result<Vec<u8>> {
    match encoding {
        RawTransactionEncoding::Base58 => bs58::decode(input)
            .into_vec()
            .map_err(|err| map_base58_error(input, err)),
        RawTransactionEncoding::Base64 => BASE64_STANDARD
            .decode(input.as_bytes())
            .map_err(|err| anyhow!("Base64 解码失败: {err}")),
    }
}

fn map_base58_error(input: &str, err: Base58Error) -> anyhow::Error {
    let base_message = match err {
        Base58Error::InvalidCharacter { character, index } => {
            format!("Base58 解码失败: 第 {index} 位包含非法字符 `{character}`")
        }
        other => format!("Base58 解码失败: {other}"),
    };

    if input.contains(['+', '/', '=']) {
        anyhow!("{base_message}。检测到 Base64 特征字符，可能需要尝试 Base64 编码")
    } else {
        anyhow!(base_message)
    }
}

impl TransactionSummary {
    pub fn from_transaction(tx: &VersionedTransaction, plan: &MessageAccountPlan) -> Self {
        let message = &tx.message;
        let lookup_locations = build_lookup_locations(&plan.address_lookups);
        let static_accounts = plan
            .static_accounts
            .iter()
            .enumerate()
            .map(|(index, key)| AccountKeySummary {
                index,
                pubkey: key.to_string(),
                signer: message.is_signer(index),
                writable: message.is_maybe_writable(index, None),
            })
            .collect();

        let instructions = message
            .instructions()
            .iter()
            .enumerate()
            .map(|(idx, ix)| InstructionSummary {
                index: idx,
                program: classify_account_reference(
                    message,
                    ix.program_id_index as usize,
                    plan,
                    &lookup_locations,
                ),
                accounts: ix
                    .accounts
                    .iter()
                    .map(|account_index| {
                        classify_account_reference(
                            message,
                            *account_index as usize,
                            plan,
                            &lookup_locations,
                        )
                    })
                    .collect(),
                data_length: ix.data.len(),
            })
            .collect();

        let address_table_lookups = plan
            .address_lookups
            .iter()
            .map(|lookup| AddressLookupSummary {
                account_key: lookup.account_key.to_string(),
                writable_indexes: lookup.writable_indexes.clone(),
                readonly_indexes: lookup.readonly_indexes.clone(),
            })
            .collect();

        TransactionSummary {
            signatures: tx.signatures.iter().map(|sig| sig.to_string()).collect(),
            recent_blockhash: message.recent_blockhash().to_string(),
            static_accounts,
            instructions,
            address_table_lookups,
        }
    }
}

/// 将账户索引分类为静态账户或 lookup table 账户
///
/// Solana V0 交易中的账户索引规则：
/// - 指令（Instruction）中的账户索引是**全局索引**，范围是 [0, total_accounts)
/// - total_accounts = static_accounts.len() + lookup_accounts.len()
///
/// 全局索引到账户的映射规则：
/// 1. 索引 [0, static_accounts.len()) 对应静态账户
/// 2. 索引 [static_accounts.len(), total_accounts) 对应 lookup table 账户
///
/// Lookup table 账户的顺序：
/// - 对于每个 address_table_lookup（按交易中的顺序）：
///   a. 首先是该 table 的所有 writable_indexes 对应的账户
///   b. 然后是该 table 的所有 readonly_indexes 对应的账户
///
/// # 参数
/// * `message` - 交易消息，用于查询账户属性
/// * `index` - 指令中引用的全局账户索引
/// * `plan` - 账户计划，包含静态账户和 lookup table 信息
/// * `lookup_locations` - lookup table 账户的位置映射表
///
/// # 返回值
/// 返回账户引用摘要，包含账户的来源、公钥、签名者和可写属性
fn classify_account_reference(
    message: &VersionedMessage,
    index: usize,
    plan: &MessageAccountPlan,
    lookup_locations: &[LookupLocation],
) -> AccountReferenceSummary {
    if index < plan.static_accounts.len() {
        AccountReferenceSummary {
            index,
            pubkey: Some(plan.static_accounts[index].to_string()),
            signer: message.is_signer(index),
            writable: message.is_maybe_writable(index, None),
            source: AccountSourceSummary::Static,
        }
    } else {
        let lookup_index = index - plan.static_accounts.len();
        let Some(location) = lookup_locations.get(lookup_index) else {
            return AccountReferenceSummary {
                index,
                pubkey: None,
                signer: false,
                writable: false,
                source: AccountSourceSummary::Unknown,
            };
        };
        AccountReferenceSummary {
            index,
            pubkey: None,
            signer: false,
            writable: location.writable,
            source: AccountSourceSummary::Lookup {
                table_account: location.table_account.to_string(),
                lookup_index: location.table_index,
                writable: location.writable,
            },
        }
    }
}

fn build_address_lookup_plan(message: &VersionedMessage) -> Vec<AddressLookupPlan> {
    message
        .address_table_lookups()
        .map(|lookups| {
            lookups
                .iter()
                .map(|lookup| AddressLookupPlan {
                    account_key: lookup.account_key,
                    writable_indexes: lookup.writable_indexes.clone(),
                    readonly_indexes: lookup.readonly_indexes.clone(),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 构建 lookup table 账户位置映射表
///
/// 此函数按照 Solana V0 交易规范构建账户索引映射。在 V0 交易中，账户的全局顺序为：
/// 1. 静态账户 (static_account_keys)
/// 2. 来自地址查找表的账户，按以下顺序：
///    - 对于每个 lookup table（按在 address_table_lookups 中的顺序）：
///      a. 该 table 的所有 writable_indexes 对应的账户
///      b. 该 table 的所有 readonly_indexes 对应的账户
///
/// 返回的 Vec<LookupLocation> 的索引对应于 (全局账户索引 - 静态账户数量)
///
/// # 参数
/// * `plan` - 地址查找表计划列表，顺序必须与交易中的 address_table_lookups 一致
///
/// # 返回值
/// 返回一个按全局账户索引排序的 lookup location 列表
fn build_lookup_locations(plan: &[AddressLookupPlan]) -> Vec<LookupLocation> {
    let mut locations = Vec::new();

    // 遍历每个 lookup table（保持交易中的顺序）
    for entry in plan {
        // 先添加所有可写账户（Solana 规范要求的顺序）
        for &idx in &entry.writable_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx, // lookup table 内部的索引
                writable: true,
            });
        }
    }

    // 遍历每个 lookup table（保持交易中的顺序）
    for entry in plan {
        // 再添加所有只读账户
        for &idx in &entry.readonly_indexes {
            locations.push(LookupLocation {
                table_account: entry.account_key,
                table_index: idx, // lookup table 内部的索引
                writable: false,
            });
        }
    }

    locations
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
    use base64::Engine;
    #[allow(deprecated)]
    use solana_sdk::system_instruction;
    use solana_sdk::{
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        transaction::Transaction,
    };

    fn sample_transaction() -> (VersionedTransaction, Pubkey) {
        let payer = Keypair::new();
        let recipient = Pubkey::new_unique();
        let blockhash = Hash::new_unique();
        let instruction = system_instruction::transfer(&payer.pubkey(), &recipient, 42);
        let message = Message::new(&[instruction], Some(&payer.pubkey()));
        let transaction = Transaction::new(&[&payer], message, blockhash);
        (VersionedTransaction::from(transaction), payer.pubkey())
    }

    #[test]
    fn parse_base64_transaction() {
        let (versioned, payer) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let base64 = BASE64_STANDARD.encode(&bytes);

        let parsed = parse_raw_transaction(&base64).expect("parse base64");
        assert_eq!(parsed.encoding, RawTransactionEncoding::Base64);
        assert_eq!(parsed.summary.signatures.len(), 1);
        assert_eq!(parsed.summary.static_accounts.len(), 3);
        assert_eq!(parsed.summary.instructions.len(), 1);
        assert_eq!(parsed.summary.static_accounts[0].pubkey, payer.to_string());
    }

    #[test]
    fn parse_base58_transaction() {
        let (versioned, _) = sample_transaction();
        let bytes = bincode::serialize(&versioned).unwrap();
        let base58 = bs58::encode(&bytes).into_string();

        let parsed = parse_raw_transaction(&base58).expect("parse base58");
        assert_eq!(parsed.encoding, RawTransactionEncoding::Base58);
        assert_eq!(parsed.summary.instructions.len(), 1);
    }

    #[test]
    fn test_build_lookup_locations_ordering() {
        // 创建两个 lookup table，每个都有可写和只读索引
        let table1 = Pubkey::new_unique();
        let table2 = Pubkey::new_unique();

        let plan = vec![
            AddressLookupPlan {
                account_key: table1,
                writable_indexes: vec![0, 1], // table1 的可写索引: 0, 1
                readonly_indexes: vec![2, 3], // table1 的只读索引: 2, 3
            },
            AddressLookupPlan {
                account_key: table2,
                writable_indexes: vec![5, 6], // table2 的可写索引: 5, 6
                readonly_indexes: vec![7],    // table2 的只读索引: 7
            },
        ];

        let locations = build_lookup_locations(&plan);

        // 验证顺序符合 Solana 规范（按新的实现）：
        // 先所有表的 writable 索引，再所有表的 readonly 索引
        // 全局索引 0: table1[0] writable
        // 全局索引 1: table1[1] writable
        // 全局索引 2: table2[5] writable
        // 全局索引 3: table2[6] writable
        // 全局索引 4: table1[2] readonly
        // 全局索引 5: table1[3] readonly
        // 全局索引 6: table2[7] readonly

        assert_eq!(locations.len(), 7, "应该有 7 个 lookup 账户");

        // 验证 table1 的可写账户
        assert_eq!(locations[0].table_account, table1);
        assert_eq!(locations[0].table_index, 0);
        assert_eq!(locations[0].writable, true);

        assert_eq!(locations[1].table_account, table1);
        assert_eq!(locations[1].table_index, 1);
        assert_eq!(locations[1].writable, true);

        // 验证 table2 的可写账户
        assert_eq!(locations[2].table_account, table2);
        assert_eq!(locations[2].table_index, 5);
        assert_eq!(locations[2].writable, true);

        assert_eq!(locations[3].table_account, table2);
        assert_eq!(locations[3].table_index, 6);
        assert_eq!(locations[3].writable, true);

        // 验证 table1 的只读账户
        assert_eq!(locations[4].table_account, table1);
        assert_eq!(locations[4].table_index, 2);
        assert_eq!(locations[4].writable, false);

        assert_eq!(locations[5].table_account, table1);
        assert_eq!(locations[5].table_index, 3);
        assert_eq!(locations[5].writable, false);

        // 验证 table2 的只读账户
        assert_eq!(locations[6].table_account, table2);
        assert_eq!(locations[6].table_index, 7);
        assert_eq!(locations[6].writable, false);
    }

    #[test]
    fn test_build_lookup_locations_empty() {
        let locations = build_lookup_locations(&[]);
        assert_eq!(
            locations.len(),
            0,
            "空的 lookup plan 应该返回空的 locations"
        );
    }

    #[test]
    fn test_build_lookup_locations_single_table() {
        let table = Pubkey::new_unique();
        let plan = vec![AddressLookupPlan {
            account_key: table,
            writable_indexes: vec![10],
            readonly_indexes: vec![20, 21],
        }];

        let locations = build_lookup_locations(&plan);

        assert_eq!(locations.len(), 3);

        // 可写账户应该在前
        assert_eq!(locations[0].table_index, 10);
        assert_eq!(locations[0].writable, true);

        // 只读账户在后
        assert_eq!(locations[1].table_index, 20);
        assert_eq!(locations[1].writable, false);

        assert_eq!(locations[2].table_index, 21);
        assert_eq!(locations[2].writable, false);
    }

    #[test]
    fn test_classify_account_reference_with_lookups() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // 创建 3 个静态账户
        let static_key1 = Pubkey::new_unique();
        let static_key2 = Pubkey::new_unique();
        let static_key3 = Pubkey::new_unique();
        let static_accounts = vec![static_key1, static_key2, static_key3];

        // 创建 2 个 lookup table
        let lookup_table1 = Pubkey::new_unique();
        let lookup_table2 = Pubkey::new_unique();

        let address_table_lookups = vec![
            MessageAddressTableLookup {
                account_key: lookup_table1,
                writable_indexes: vec![0, 1], // 2 个可写
                readonly_indexes: vec![2],    // 1 个只读
            },
            MessageAddressTableLookup {
                account_key: lookup_table2,
                writable_indexes: vec![3],    // 1 个可写
                readonly_indexes: vec![4, 5], // 2 个只读
            },
        ];

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: Hash::default(),
            account_keys: static_accounts.clone(),
            address_table_lookups,
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: static_accounts.clone(),
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // 验证账户索引映射（按新的顺序：先所有 writable，再所有 readonly）
        // 索引 0-2: 静态账户
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(static_key1.to_string()));
        assert!(matches!(ref0.source, AccountSourceSummary::Static));

        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        assert_eq!(ref2.pubkey, Some(static_key3.to_string()));
        assert!(matches!(ref2.source, AccountSourceSummary::Static));

        // 索引 3: lookup_table1[0] writable
        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        assert_eq!(ref3.index, 3);
        match &ref3.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table1.to_string());
                assert_eq!(*lookup_index, 0);
                assert_eq!(*writable, true);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 索引 4: lookup_table1[1] writable
        let ref4 = classify_account_reference(&message, 4, &plan, &lookup_locations);
        match &ref4.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table1.to_string());
                assert_eq!(*lookup_index, 1);
                assert_eq!(*writable, true);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 索引 5: lookup_table2[3] writable (新顺序：table2 的 writable 在 table1 writable 之后)
        let ref5 = classify_account_reference(&message, 5, &plan, &lookup_locations);
        match &ref5.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table2.to_string());
                assert_eq!(*lookup_index, 3);
                assert_eq!(*writable, true);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 索引 6: lookup_table1[2] readonly (所有 writable 完成后开始 readonly)
        let ref6 = classify_account_reference(&message, 6, &plan, &lookup_locations);
        match &ref6.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table1.to_string());
                assert_eq!(*lookup_index, 2);
                assert_eq!(*writable, false);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 索引 7: lookup_table2[4] readonly
        let ref7 = classify_account_reference(&message, 7, &plan, &lookup_locations);
        match &ref7.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table2.to_string());
                assert_eq!(*lookup_index, 4);
                assert_eq!(*writable, false);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 索引 8: lookup_table2[5] readonly
        let ref8 = classify_account_reference(&message, 8, &plan, &lookup_locations);
        match &ref8.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table2.to_string());
                assert_eq!(*lookup_index, 5);
                assert_eq!(*writable, false);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 验证总账户数量：3 个静态 + 6 个 lookup
        assert_eq!(lookup_locations.len(), 6);
    }

    #[test]
    fn test_account_ordering_edge_cases() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // 测试只有 readonly 索引的情况
        let static_key = Pubkey::new_unique();
        let lookup_table = Pubkey::new_unique();

        let address_table_lookups = vec![MessageAddressTableLookup {
            account_key: lookup_table,
            writable_indexes: vec![],        // 没有可写账户
            readonly_indexes: vec![0, 1, 2], // 只有只读账户
        }];

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 1,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: Hash::default(),
            account_keys: vec![static_key],
            address_table_lookups: address_table_lookups.clone(),
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![static_key],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // 验证只有 readonly 索引时的顺序
        assert_eq!(lookup_locations.len(), 3);

        // 索引 1 应该是 lookup_table[0] readonly
        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        match &ref1.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 0);
                assert_eq!(*writable, false);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 索引 2 应该是 lookup_table[1] readonly
        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        match &ref2.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 1);
                assert_eq!(*writable, false);
            }
            _ => panic!("期望是 Lookup 源"),
        }

        // 索引 3 应该是 lookup_table[2] readonly
        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        match &ref3.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 2);
                assert_eq!(*writable, false);
            }
            _ => panic!("期望是 Lookup 源"),
        }
    }

    #[test]
    fn test_two_writable_signers_scenario() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // 测试场景：2 个可写签名者 + lookup table
        // num_readonly_signed_accounts = 0 意味着所有签名者都是可写的
        let signer1 = Pubkey::new_unique();
        let signer2 = Pubkey::new_unique();
        let lookup_table = Pubkey::new_unique();

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 2,        // 2 个签名者
                num_readonly_signed_accounts: 0,   // 0 个只读签名者（都是可写的）
                num_readonly_unsigned_accounts: 0, // 0 个只读非签名者
            },
            recent_blockhash: Hash::default(),
            account_keys: vec![signer1, signer2], // 只有 2 个可写签名者
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: lookup_table,
                writable_indexes: vec![0, 1],
                readonly_indexes: vec![2],
            }],
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![signer1, signer2],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // 验证静态账户
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(signer1.to_string()));
        assert_eq!(ref0.signer, true, "索引 0 应该是签名者");
        assert_eq!(ref0.writable, true, "索引 0 应该可写");
        assert!(matches!(ref0.source, AccountSourceSummary::Static));

        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        assert_eq!(ref1.pubkey, Some(signer2.to_string()));
        assert_eq!(ref1.signer, true, "索引 1 应该是签名者");
        assert_eq!(ref1.writable, true, "索引 1 应该可写");
        assert!(matches!(ref1.source, AccountSourceSummary::Static));

        // 验证 lookup table 账户从索引 2 开始
        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        match &ref2.source {
            AccountSourceSummary::Lookup {
                table_account,
                lookup_index,
                writable,
            } => {
                assert_eq!(*table_account, lookup_table.to_string());
                assert_eq!(*lookup_index, 0, "应该映射到 lookup table 的索引 0");
                assert_eq!(*writable, true, "应该是可写的");
            }
            _ => panic!("期望索引 2 是 Lookup 源"),
        }

        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        match &ref3.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 1, "应该映射到 lookup table 的索引 1");
                assert_eq!(*writable, true, "应该是可写的");
            }
            _ => panic!("期望索引 3 是 Lookup 源"),
        }

        let ref4 = classify_account_reference(&message, 4, &plan, &lookup_locations);
        match &ref4.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 2, "应该映射到 lookup table 的索引 2");
                assert_eq!(*writable, false, "应该是只读的");
            }
            _ => panic!("期望索引 4 是 Lookup 源"),
        }

        // 验证总账户数量：2 个静态 + 3 个 lookup
        assert_eq!(lookup_locations.len(), 3);
    }

    #[test]
    fn test_mixed_signers_with_lookups() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::{v0::MessageAddressTableLookup, MessageHeader};

        // 测试场景：混合签名者（1 可写 + 1 只读）+ 非签名者 + lookup table
        let writable_signer = Pubkey::new_unique();
        let readonly_signer = Pubkey::new_unique();
        let writable_non_signer = Pubkey::new_unique();
        let readonly_non_signer = Pubkey::new_unique();
        let lookup_table = Pubkey::new_unique();

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 2,        // 2 个签名者
                num_readonly_signed_accounts: 1,   // 1 个只读签名者
                num_readonly_unsigned_accounts: 1, // 1 个只读非签名者
            },
            recent_blockhash: Hash::default(),
            // 按照 Solana 规范排序：可写签名者 -> 只读签名者 -> 可写非签名者 -> 只读非签名者
            account_keys: vec![
                writable_signer,     // 索引 0
                readonly_signer,     // 索引 1
                writable_non_signer, // 索引 2
                readonly_non_signer, // 索引 3
            ],
            address_table_lookups: vec![MessageAddressTableLookup {
                account_key: lookup_table,
                writable_indexes: vec![0],
                readonly_indexes: vec![1],
            }],
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![
                writable_signer,
                readonly_signer,
                writable_non_signer,
                readonly_non_signer,
            ],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // 验证静态账户
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(writable_signer.to_string()));
        assert_eq!(ref0.signer, true);
        assert_eq!(ref0.writable, true);

        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        assert_eq!(ref1.pubkey, Some(readonly_signer.to_string()));
        assert_eq!(ref1.signer, true);
        assert_eq!(ref1.writable, false);

        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        assert_eq!(ref2.pubkey, Some(writable_non_signer.to_string()));
        assert_eq!(ref2.signer, false);
        assert_eq!(ref2.writable, true);

        let ref3 = classify_account_reference(&message, 3, &plan, &lookup_locations);
        assert_eq!(ref3.pubkey, Some(readonly_non_signer.to_string()));
        assert_eq!(ref3.signer, false);
        assert_eq!(ref3.writable, false);

        // 验证 lookup table 账户从索引 4 开始
        let ref4 = classify_account_reference(&message, 4, &plan, &lookup_locations);
        match &ref4.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 0);
                assert_eq!(*writable, true);
            }
            _ => panic!("期望索引 4 是 Lookup 源"),
        }

        let ref5 = classify_account_reference(&message, 5, &plan, &lookup_locations);
        match &ref5.source {
            AccountSourceSummary::Lookup {
                lookup_index,
                writable,
                ..
            } => {
                assert_eq!(*lookup_index, 1);
                assert_eq!(*writable, false);
            }
            _ => panic!("期望索引 5 是 Lookup 源"),
        }
    }

    #[test]
    fn test_only_signers_no_lookups() {
        use solana_message::{v0, VersionedMessage};
        use solana_sdk::hash::Hash;
        use solana_sdk::message::MessageHeader;

        // 测试场景：只有签名者，没有 lookup table
        let signer1 = Pubkey::new_unique();
        let signer2 = Pubkey::new_unique();

        let message = VersionedMessage::V0(v0::Message {
            header: MessageHeader {
                num_required_signatures: 2,
                num_readonly_signed_accounts: 0,
                num_readonly_unsigned_accounts: 0,
            },
            recent_blockhash: Hash::default(),
            account_keys: vec![signer1, signer2],
            address_table_lookups: vec![], // 没有 lookup table
            instructions: vec![],
        });

        let plan = MessageAccountPlan {
            static_accounts: vec![signer1, signer2],
            address_lookups: build_address_lookup_plan(&message),
        };

        let lookup_locations = build_lookup_locations(&plan.address_lookups);

        // 验证没有 lookup 账户
        assert_eq!(lookup_locations.len(), 0);

        // 验证静态账户
        let ref0 = classify_account_reference(&message, 0, &plan, &lookup_locations);
        assert_eq!(ref0.pubkey, Some(signer1.to_string()));
        assert_eq!(ref0.signer, true);
        assert_eq!(ref0.writable, true);

        let ref1 = classify_account_reference(&message, 1, &plan, &lookup_locations);
        assert_eq!(ref1.pubkey, Some(signer2.to_string()));
        assert_eq!(ref1.signer, true);
        assert_eq!(ref1.writable, true);

        // 验证超出范围的索引返回 Unknown
        let ref2 = classify_account_reference(&message, 2, &plan, &lookup_locations);
        assert!(matches!(ref2.source, AccountSourceSummary::Unknown));
    }
}
