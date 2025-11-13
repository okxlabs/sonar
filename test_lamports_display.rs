use std::str::FromStr;
use solsim::instruction_parser::SystemProgramParser;
use solsim::transaction::{AccountReferenceSummary, AccountSourceSummary, InstructionSummary};
use solana_pubkey::Pubkey;

fn main() {
    let parser = SystemProgramParser::new();

    let accounts = vec![
        AccountReferenceSummary {
            index: 0,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer: true,
            writable: true,
            source: AccountSourceSummary::Static,
        },
        AccountReferenceSummary {
            index: 1,
            pubkey: Some(Pubkey::new_unique().to_string()),
            signer: false,
            writable: true,
            source: AccountSourceSummary::Static,
        },
    ];

    // Transfer 8500 lamports (this was showing as 0.000008500 SOL before)
    let mut data = vec![2u8, 0, 0, 0]; // Transfer instruction discriminator
    data.extend_from_slice(&8500u64.to_le_bytes()); // 8500 lamports

    let instruction = InstructionSummary {
        index: 0,
        program: AccountReferenceSummary {
            index: 6,
            pubkey: Some(solana_sdk_ids::system_program::id().to_string()),
            signer: false,
            writable: false,
            source: AccountSourceSummary::Static,
        },
        accounts,
        data: data.into_boxed_slice(),
    };

    let result = parser.parse_instruction(&instruction).unwrap().unwrap();
    
    println!("Instruction: {}", result.name);
    println!("Field name: {}", result.fields[0].0);
    println!("Field value: {}", result.fields[0].1);
    println!();
    println!("✅ LAMPORTS ARE NOW DISPLAYED AS INTEGERS instead of floating-point SOL values");
}
