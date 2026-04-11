use anyhow::Result;
use hex::encode as hex_encode;
use solana_pubkey::Pubkey;
use sonar_idl::IdlValue;

use super::{InstructionParser, ParsedField, ParsedInstruction};
use crate::core::transaction::InstructionSummary;

const MEMO_PROGRAM_ID: &str = "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr";

define_parser!(MemoProgramParser, MEMO_PROGRAM_ID);

impl InstructionParser for MemoProgramParser {
    fn program_id(&self) -> &Pubkey {
        &self.program_id
    }

    fn parse_instruction(
        &self,
        instruction: &InstructionSummary,
    ) -> Result<Option<ParsedInstruction>> {
        let memo_fields = parse_memo_fields(&instruction.data);
        let account_names =
            (0..instruction.accounts.len()).map(|index| format!("signer_{}", index)).collect();

        Ok(Some(ParsedInstruction {
            name: "Memo".to_string(),
            fields: memo_fields.into(),
            account_names,
        }))
    }
}

fn parse_memo_fields(data: &[u8]) -> Vec<ParsedField> {
    match std::str::from_utf8(data) {
        Ok(memo) => vec![ParsedField { name: "memo".into(), value: IdlValue::String(memo.into()) }],
        Err(_) => vec![
            ParsedField {
                name: "memo".into(),
                value: IdlValue::String(String::from_utf8_lossy(data).into_owned().into()),
            },
            ParsedField {
                name: "memo_hex".into(),
                value: IdlValue::String(hex_encode(data).into()),
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{AccountReferenceSummary, AccountSourceSummary};

    fn create_test_instruction(data: Vec<u8>, signer_count: usize) -> InstructionSummary {
        let accounts = (0..signer_count)
            .map(|index| AccountReferenceSummary {
                index,
                pubkey: Some(Pubkey::new_unique().to_string()),
                signer: true,
                writable: false,
                source: AccountSourceSummary::Static,
            })
            .collect();

        InstructionSummary {
            index: 0,
            program: AccountReferenceSummary {
                index: signer_count,
                pubkey: Some(MEMO_PROGRAM_ID.to_string()),
                signer: false,
                writable: false,
                source: AccountSourceSummary::Static,
            },
            accounts,
            data: data.into_boxed_slice(),
        }
    }

    #[test]
    fn test_program_id() {
        let parser = MemoProgramParser::new();
        let expected_id = Pubkey::from_str_const(MEMO_PROGRAM_ID);
        assert_eq!(parser.program_id(), &expected_id);
    }

    #[test]
    fn test_utf8_memo_instruction_parsing() {
        let parser = MemoProgramParser::new();
        let instruction = create_test_instruction(b"hello memo".to_vec(), 1);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Memo");
        assert!(
            parsed.fields.iter().any(|field| field.name == "memo" && field.value == "hello memo")
        );
        assert_eq!(parsed.account_names, vec!["signer_0"]);
    }

    #[test]
    fn test_empty_memo_instruction_parsing() {
        let parser = MemoProgramParser::new();
        let instruction = create_test_instruction(vec![], 0);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.name, "Memo");
        assert!(parsed.fields.iter().any(|field| field.name == "memo" && field.value == ""));
        assert!(parsed.account_names.is_empty());
    }

    #[test]
    fn test_multi_signer_memo_instruction_parsing() {
        let parser = MemoProgramParser::new();
        let instruction = create_test_instruction(b"multisig memo".to_vec(), 3);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert_eq!(parsed.account_names, vec!["signer_0", "signer_1", "signer_2"]);
    }

    #[test]
    fn test_non_utf8_memo_instruction_uses_hex_fallback() {
        let parser = MemoProgramParser::new();
        let instruction = create_test_instruction(vec![0xf0, 0x28, 0x8c, 0x28], 1);

        let parsed = parser.parse_instruction(&instruction).unwrap().unwrap();
        assert!(
            parsed.fields.iter().any(|field| field.name == "memo_hex" && field.value == "f0288c28")
        );
        assert!(parsed.fields.iter().any(|field| field.name == "memo"));
    }
}
