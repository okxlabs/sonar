//! Colored text rendering for the `account` subcommand.

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use super::terminal::render_section_title;

const INDENT: &str = "  ";
const INDENT_L2: &str = "    ";
const DIM_GRAY: colored::CustomColor = colored::CustomColor { r: 128, g: 128, b: 128 };
const INFO_LABEL_WIDTH: usize = 12;

const WRAPPER_FIELDS: &[&str] = &["lamports", "space", "owner", "executable", "rentEpoch"];
const METADATA_SKIP_FIELDS: &[&str] =
    &["lamports", "space", "owner", "executable", "rentEpoch", "metaplexMetadata"];

pub(crate) fn render_account_text(
    pubkey: &str,
    account: &solana_account::Account,
    account_type: &str,
    decoded: &Value,
    metadata: Option<&Value>,
) -> Result<()> {
    render_account_info(pubkey, account);
    render_account_data(account_type, decoded, METADATA_SKIP_FIELDS);

    if let Some(meta) = metadata {
        render_metadata_section(meta);
    }

    println!();
    Ok(())
}

fn render_account_info(pubkey: &str, account: &solana_account::Account) {
    render_section_title("Account Info");

    let sol = account.lamports as f64 / 1_000_000_000.0;
    let owner_str = account.owner.to_string();
    let owner_display = match known_program_name(&owner_str) {
        Some(name) => format!("{} ({})", owner_str, name),
        None => owner_str,
    };

    print_info_row("Address", pubkey);
    print_info_row(
        "Balance",
        &format!("{:.9} SOL ({} lamports)", sol, format_with_commas(account.lamports)),
    );
    print_info_row("Owner", &owner_display);
    print_info_row("Space", &format!("{} bytes", format_with_commas(account.data.len() as u64)));
    print_info_row("Executable", if account.executable { "Yes" } else { "No" });
}

fn print_info_row(label: &str, value: &str) {
    let padded = format!("{:<w$}", label, w = INFO_LABEL_WIDTH);
    println!("{}{}   {}", INDENT, padded.custom_color(DIM_GRAY), value);
}

fn render_account_data(account_type: &str, decoded: &Value, extra_skip: &[&str]) {
    if account_type == "Address Lookup Table" {
        render_lookup_table_data(decoded);
        return;
    }

    let data = decoded.get("data").unwrap_or(decoded);
    let extensions = data.get("extensions").and_then(Value::as_array);
    let skip: Vec<&str> = if decoded.get("data").is_some() {
        extra_skip.iter().copied().chain(["extensions"]).collect()
    } else {
        WRAPPER_FIELDS
            .iter()
            .copied()
            .chain(extra_skip.iter().copied())
            .chain(["extensions"])
            .collect()
    };

    render_section_title(&format!("Account Data ({})", account_type));

    if let Some(obj) = data.as_object() {
        render_kv_pairs(obj, INDENT, &skip);
    }

    if let Some(exts) = extensions {
        if !exts.is_empty() {
            render_section_title("Extensions");
            render_extensions(exts);
        }
    }
}

fn render_metadata_section(metadata: &Value) {
    let data = metadata.get("data").unwrap_or(metadata);

    render_section_title("Metaplex Metadata");

    if let Some(obj) = data.as_object() {
        render_kv_pairs(obj, INDENT, &[]);
    }
}

fn render_lookup_table_data(decoded: &Value) {
    let data = decoded.get("data").unwrap_or(decoded);

    render_section_title("Account Data (Address Lookup Table)");

    if let Some(meta) = data.get("meta") {
        if let Some(obj) = meta.as_object() {
            render_kv_pairs(obj, INDENT, &["_padding"]);
        }
    }

    if let Some(Value::Array(addresses)) = data.get("addresses") {
        render_section_title(&format!("Addresses ({})", addresses.len()));
        render_address_list(addresses);
    }
}

fn render_kv_pairs(obj: &serde_json::Map<String, Value>, indent: &str, skip_keys: &[&str]) {
    let entries: Vec<_> = obj.iter().filter(|(k, _)| !skip_keys.contains(&k.as_str())).collect();

    if entries.is_empty() {
        return;
    }

    let max_key_len = entries.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

    for (key, val) in entries {
        let padded_key = format!("{:<w$}", key, w = max_key_len);
        match val {
            Value::Object(inner) if !inner.is_empty() => {
                println!("{}{}", indent, padded_key.custom_color(DIM_GRAY));
                let nested = format!("{}  ", indent);
                render_kv_pairs(inner, &nested, &[]);
            }
            Value::Array(arr) if !arr.is_empty() && arr.iter().all(Value::is_object) => {
                println!("{}{}", indent, padded_key.custom_color(DIM_GRAY));
                let nested = format!("{}  ", indent);
                for (i, item) in arr.iter().enumerate() {
                    if let Some(inner_obj) = item.as_object() {
                        let label = format!("[{}]", i + 1).custom_color(DIM_GRAY).to_string();
                        println!("{}{}", nested, label);
                        let deep = format!("{}  ", nested);
                        render_kv_pairs(inner_obj, &deep, &[]);
                    }
                }
            }
            _ => {
                println!("{}{}   {}", indent, padded_key.custom_color(DIM_GRAY), format_value(val));
            }
        }
    }
}

fn render_address_list(addresses: &[Value]) {
    if addresses.is_empty() {
        return;
    }

    let index_width = if addresses.len() >= 100 { 3 } else { 2 };

    for (i, addr) in addresses.iter().enumerate() {
        if let Some(s) = addr.as_str() {
            let label = format!("[{:>w$}]", i, w = index_width).custom_color(DIM_GRAY).to_string();
            println!("{}{} {}", INDENT, label, s);
        }
    }
}

fn render_extensions(extensions: &[Value]) {
    for (i, ext) in extensions.iter().enumerate() {
        if i > 0 {
            println!();
        }

        let type_name = ext.get("type").and_then(Value::as_str).unwrap_or("Unknown");

        let label = format!("[{}]", i + 1).custom_color(DIM_GRAY);
        println!("{}{} {}", INDENT, label, type_name.bold());

        if let Some(data) = ext.get("data") {
            if let Some(obj) = data.as_object() {
                if !obj.is_empty() {
                    render_kv_pairs(obj, INDENT_L2, &[]);
                }
            } else if data.is_null() {
                println!("{}(unsupported)", INDENT_L2);
            }
        }
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => {
            if s.len() > 120 {
                format!("{}…", &s[..120])
            } else {
                s.to_string()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "\u{2013}".to_string(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(_) => serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
    }
}

fn format_with_commas(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}

fn known_program_name(pubkey: &str) -> Option<&'static str> {
    match pubkey {
        "11111111111111111111111111111111" => Some("System Program"),
        "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA" => Some("Token Program"),
        "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb" => Some("Token-2022 Program"),
        "BPFLoaderUpgradeab1e11111111111111111111111" => Some("BPF Loader Upgradeable"),
        "AddressLookupTab1e1111111111111111111111111" => Some("Address Lookup Table Program"),
        "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL" => Some("Associated Token Program"),
        "ComputeBudget111111111111111111111111111111" => Some("Compute Budget Program"),
        "Vote111111111111111111111111111111111111111" => Some("Vote Program"),
        "Stake11111111111111111111111111111111111111" => Some("Stake Program"),
        "metaqbxxUerdq28cj1RbAWkYQm3ybzjb6a8bt518x1s" => Some("Metaplex Token Metadata"),
        _ => None,
    }
}
