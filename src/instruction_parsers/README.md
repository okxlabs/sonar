# Instruction Parsers

This directory contains modular instruction parsers for various Solana programs.

## Structure

- `mod.rs` - Main module containing base types and parser registry
- `system_program.rs` - Parser for the Solana System Program
- `template.rs` - Template for creating new program parsers

## Adding a New Program Parser

### Step 1: Create the Parser File

Copy `template.rs` to a new file named after your program:

```bash
cp template.rs my_program.rs
```

### Step 2: Implement the Parser

Edit `my_program.rs`:

1. **Rename the parser struct**:
   - Replace `TemplateProgramParser` with `MyProgramParser`
   - Replace `template_program_parser` with `my_program_parser`

2. **Set the correct program ID**:
   ```rust
   pub fn new() -> Self {
       Self { program_id: Pubkey::from_str_const("YourProgramId1111111111111111111111111111111") }
   }
   ```

3. **Implement instruction parsing logic**:
   ```rust
   fn parse_instruction(&self, instruction: &InstructionSummary) -> Result<Option<ParsedInstruction>> {
       // Read instruction discriminator
       if instruction.data.len() < 4 {
           return Ok(None);
       }
       
       let instruction_id = u32::from_le_bytes([...]);
       let data = &instruction.data[4..];
       
       match instruction_id {
           0 => parse_my_instruction(data, instruction),
           _ => Ok(None),
       }
   }
   ```

### Step 3: Register the Parser

Edit `mod.rs`:

1. **Add module declaration** (after existing `mod` statements):
```rust
mod system_program;
mod my_program;  // Add this line
```

2. **Add public export**:
```rust
mod system_program;
mod my_program;

pub use system_program::SystemProgramParser;
pub use my_program::MyProgramParser;  // Add this line
```

3. **Register in the registry constructor**:
```rust
pub fn new() -> Self {
    let mut registry = Self { parsers: HashMap::new() };

    // Register default parsers
    let system_parser = SystemProgramParser::new();
    registry.parsers.insert(*system_parser.program_id(), Box::new(system_parser));
    
    // Register your parser
    let my_program_parser = MyProgramParser::new();
    registry.parsers.insert(*my_program_parser.program_id(), Box::new(my_program_parser));

    registry
}
```

## Best Practices

### 1. Instruction Discriminator Pattern

Most programs use 4-byte discriminators at the start of instruction data:

```rust
if instruction.data.len() < 4 {
    return Ok(None);
}

let instruction_id = u32::from_le_bytes([
    instruction.data[0],
    instruction.data[1],
    instruction.data[2],
    instruction.data[3],
]);
```

### 2. Data Parsing Pattern

Parse data based on the instruction format. Common patterns:

```rust
// 8-byte number
let amount = u64::from_le_bytes([
    data[0], data[1], data[2], data[3],
    data[4], data[5], data[6], data[7],
]);

// 32-byte pubkey
let pubkey_bytes: [u8; 32] = data[0..32].try_into().unwrap();
let pubkey = Pubkey::from(pubkey_bytes);

// Variable-length string with bincode length prefix
let str_length = u64::from_le_bytes([
    data[0], data[1], data[2], data[3],
    data[4], data[5], data[6], data[7],
]) as usize;
let str_bytes = &data[8..8 + str_length];
let value = String::from_utf8_lossy(str_bytes).into_owned();
```

### 3. Error Handling

Return `Ok(None)` for unknown or invalid instructions, throwing errors only for truly unexpected issues:

```rust
fn parse_my_instruction(data: &[u8]) -> Result<Option<ParsedInstruction>> {
    if data.len() < 8 {
        return Ok(None);  // Not our instruction, not an error
    }
    
    // Bad data format = error, but still recoverable
    let amount = u64::from_le_bytes(...);
    
    Ok(Some(ParsedInstruction {
        name: "MyInstruction".to_string(),
        fields: vec![("amount".to_string(), amount.to_string())],
        account_names: vec!["account1".to_string(), "account2".to_string()],
    }))
}
```

### 4. Account Names

Provide meaningful account names in the order they appear in `instruction.accounts`:

```rust
account_names: vec![
    "payer".to_string(),
    "recipient".to_string(),
    "system_program".to_string(),
]
```

### 5. Field Formatting

Be consistent with field names and formatting:

- Use lowercase with underscores for field names (`amount`, `user_pubkey`)
- Format amounts as strings but include denomination in field name (`amount_lamports`)
- Format pubkeys as base58 strings using `pubkey.to_string()`
- Use clear, descriptive field names

### 6. Testing

Always include tests for each instruction type:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_instruction_parsing() {
        let parser = MyProgramParser::new();
        
        // Create test data and accounts
        let mut data = vec![0, 0, 0, 0]; // discriminator
        data.extend_from_slice(&1_000_000_u64.to_le_bytes());
        
        let accounts = vec![...];
        let instruction = create_test_instruction(data, accounts);
        
        // Test parsing
        let result = parser.parse_instruction(&instruction).unwrap();
        assert!(result.is_some());
        
        let parsed = result.unwrap();
        assert_eq!(parsed.name, "MyInstruction");
        // ... more assertions
    }
}
```

## Example: SPL Token Parser

Here's a quick example of what an SPL Token parser might look like:

```rust
// In mod.rs
mod spl_token;
pub use spl_token::SplTokenParser;

// In spl_token.rs
pub struct SplTokenParser {
    program_id: Pubkey,
}

impl SplTokenParser {
    pub fn new() -> Self {
        Self { program_id: Pubkey::from_str_const("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA") }
    }
}

fn parse_instruction(&self, instruction: &InstructionSummary) -> Result<Option<ParsedInstruction>> {
    if instruction.data.len() < 1 {
        return Ok(None);
    }
    
    match instruction.data[0] {
        3 => parse_transfer_instruction(&instruction.data[1..], instruction),
        7 => parse_transfer_checked_instruction(&instruction.data[1..], instruction),
        // ... other instructions
        _ => Ok(None),
    }
}
```

## Debugging Tips

1. **Verify discriminators**: Use logs to check what discriminators you're receiving
2. **Check data length**: Ensure you have enough bytes before parsing
3. **Compare with spec**: Compare your parsing logic with the program's instruction format
4. **Use test transactions**: Test with known valid transactions from mainnet
5. **Log unknown instructions**: Add logging for unrecognized instruction IDs