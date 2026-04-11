//! Colored text rendering for the `account` subcommand.

use std::io::Write;

use anyhow::Result;
use colored::Colorize;
use serde_json::Value;

use super::fmt::format_with_commas;
use super::terminal::write_section_title;
use super::theme::DIM_GRAY;

const INDENT: &str = "  ";
const INDENT_L2: &str = "    ";
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
    w: &mut impl Write,
) -> Result<()> {
    render_account_info(pubkey, account, w);
    render_account_data(account_type, decoded, METADATA_SKIP_FIELDS, w);

    if let Some(meta) = metadata {
        render_metadata_section(meta, w);
    }

    let _ = writeln!(w);
    w.flush()?;
    Ok(())
}

fn render_account_info(pubkey: &str, account: &solana_account::Account, w: &mut impl Write) {
    write_section_title(w, "Account Info");

    let sol = account.lamports as f64 / 1_000_000_000.0;
    let owner_str = account.owner.to_string();
    let owner_display = match known_program_name(&owner_str) {
        Some(name) => format!("{} ({})", owner_str, name),
        None => owner_str,
    };

    print_info_row("Address", pubkey, w);
    print_info_row(
        "Balance",
        &format!("{:.9} SOL ({} lamports)", sol, format_with_commas(account.lamports)),
        w,
    );
    print_info_row("Owner", &owner_display, w);
    print_info_row("Space", &format!("{} bytes", format_with_commas(account.data.len() as u64)), w);
    print_info_row("Executable", if account.executable { "Yes" } else { "No" }, w);
}

fn print_info_row(label: &str, value: &str, w: &mut impl Write) {
    let padded = format!("{:<w$}", label, w = INFO_LABEL_WIDTH);
    let _ = writeln!(w, "{}{}   {}", INDENT, padded.custom_color(DIM_GRAY), value);
}

fn render_account_data(
    account_type: &str,
    decoded: &Value,
    extra_skip: &[&str],
    w: &mut impl Write,
) {
    if account_type == "Address Lookup Table" {
        render_lookup_table_data(decoded, w);
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

    write_section_title(w, &format!("Account Data ({})", account_type));

    if let Some(obj) = data.as_object() {
        render_kv_pairs(obj, INDENT, &skip, w);
    }

    if let Some(exts) = extensions {
        if !exts.is_empty() {
            write_section_title(w, "Extensions");
            render_extensions(exts, w);
        }
    }
}

fn render_metadata_section(metadata: &Value, w: &mut impl Write) {
    let data = metadata.get("data").unwrap_or(metadata);

    write_section_title(w, "Metaplex Metadata");

    if let Some(obj) = data.as_object() {
        render_kv_pairs(obj, INDENT, &[], w);
    }
}

fn render_lookup_table_data(decoded: &Value, w: &mut impl Write) {
    let data = decoded.get("data").unwrap_or(decoded);

    write_section_title(w, "Account Data (Address Lookup Table)");

    if let Some(meta) = data.get("meta") {
        if let Some(obj) = meta.as_object() {
            render_kv_pairs(obj, INDENT, &["_padding"], w);
        }
    }

    if let Some(Value::Array(addresses)) = data.get("addresses") {
        write_section_title(w, &format!("Addresses ({})", addresses.len()));
        render_address_list(addresses, w);
    }
}

fn render_kv_pairs(
    obj: &serde_json::Map<String, Value>,
    indent: &str,
    skip_keys: &[&str],
    w: &mut impl Write,
) {
    let entries: Vec<_> = obj.iter().filter(|(k, _)| !skip_keys.contains(&k.as_str())).collect();

    if entries.is_empty() {
        return;
    }

    let max_key_len = entries.iter().map(|(k, _)| k.len()).max().unwrap_or(0);

    for (key, val) in entries {
        let padded_key = format!("{:<w$}", key, w = max_key_len);
        match val {
            Value::Object(inner) if !inner.is_empty() => {
                let _ = writeln!(w, "{}{}", indent, padded_key.custom_color(DIM_GRAY));
                let nested = format!("{}  ", indent);
                render_kv_pairs(inner, &nested, &[], w);
            }
            Value::Array(arr) if !arr.is_empty() && arr.iter().all(Value::is_object) => {
                let _ = writeln!(w, "{}{}", indent, padded_key.custom_color(DIM_GRAY));
                let nested = format!("{}  ", indent);
                for (i, item) in arr.iter().enumerate() {
                    if let Some(inner_obj) = item.as_object() {
                        let label = format!("[{}]", i + 1).custom_color(DIM_GRAY).to_string();
                        let _ = writeln!(w, "{}{}", nested, label);
                        let deep = format!("{}  ", nested);
                        render_kv_pairs(inner_obj, &deep, &[], w);
                    }
                }
            }
            _ => {
                let _ = writeln!(
                    w,
                    "{}{}   {}",
                    indent,
                    padded_key.custom_color(DIM_GRAY),
                    format_value(val)
                );
            }
        }
    }
}

fn render_address_list(addresses: &[Value], w: &mut impl Write) {
    if addresses.is_empty() {
        return;
    }

    let index_width = if addresses.len() >= 100 { 3 } else { 2 };

    for (i, addr) in addresses.iter().enumerate() {
        if let Some(s) = addr.as_str() {
            let label = format!("[{:>w$}]", i, w = index_width).custom_color(DIM_GRAY).to_string();
            let _ = writeln!(w, "{}{} {}", INDENT, label, s);
        }
    }
}

fn render_extensions(extensions: &[Value], w: &mut impl Write) {
    for (i, ext) in extensions.iter().enumerate() {
        if i > 0 {
            let _ = writeln!(w);
        }

        let type_name = ext.get("type").and_then(Value::as_str).unwrap_or("Unknown");

        let label = format!("[{}]", i + 1).custom_color(DIM_GRAY);
        let _ = writeln!(w, "{}{} {}", INDENT, label, type_name.bold());

        if let Some(data) = ext.get("data") {
            if let Some(obj) = data.as_object() {
                if !obj.is_empty() {
                    render_kv_pairs(obj, INDENT_L2, &[], w);
                }
            } else if data.is_null() {
                let _ = writeln!(w, "{}(unsupported)", INDENT_L2);
            }
        }
    }
}

fn format_value(value: &Value) -> String {
    match value {
        Value::String(s) => super::fmt::truncate_display(s, 120),
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
