//! Solana program log parser for structured output.
//!
//! Parses runtime logs to extract structured information about program invocations,
//! log messages, compute unit consumption, and execution results.

/// Represents a parsed log entry from Solana runtime.
#[derive(Debug, Clone)]
pub enum LogEntry {
    /// Program invocation: `Program <pubkey> invoke [N]`
    Invoke { program: String, depth: u32 },
    /// Program log message: `Program log: <message>`
    Log { message: String },
    /// Program data (base64): `Program data: <base64>`
    Data { data: String },
    /// Compute units consumed: `Program <pubkey> consumed X of Y compute units`
    Consumed { _program: String, used: u64, total: u64 },
    /// Program success: `Program <pubkey> success`
    Success { _program: String },
    /// Program failure: `Program <pubkey> failed: <error>`
    Failed { _program: String, error: String },
    /// Return data: `Program return: <pubkey> <data>`
    Return { _program: String, data: String },
    /// Unrecognized log line
    Other(String),
}

/// Parse a single log line into a structured LogEntry.
pub fn parse_log_line(line: &str) -> LogEntry {
    let trimmed = line.trim();

    // Program <pubkey> invoke [N]
    if let Some(rest) = trimmed.strip_prefix("Program ") {
        if let Some(invoke_pos) = rest.find(" invoke [") {
            let program = &rest[..invoke_pos];
            if let Some(depth_start) = rest.find('[') {
                if let Some(depth_end) = rest.find(']') {
                    if let Ok(depth) = rest[depth_start + 1..depth_end].parse::<u32>() {
                        return LogEntry::Invoke { program: program.to_string(), depth };
                    }
                }
            }
        }

        // Program <pubkey> consumed X of Y compute units
        if let Some(consumed_pos) = rest.find(" consumed ") {
            let program = &rest[..consumed_pos];
            let after_consumed = &rest[consumed_pos + 10..];
            if let Some(of_pos) = after_consumed.find(" of ") {
                if let Ok(used) = after_consumed[..of_pos].parse::<u64>() {
                    let after_of = &after_consumed[of_pos + 4..];
                    if let Some(cu_pos) = after_of.find(" compute units") {
                        if let Ok(total) = after_of[..cu_pos].parse::<u64>() {
                            return LogEntry::Consumed {
                                _program: program.to_string(),
                                used,
                                total,
                            };
                        }
                    }
                }
            }
        }

        // Program <pubkey> success
        if let Some(success_pos) = rest.find(" success") {
            // Make sure "success" is at the end (not part of a longer word)
            let after_success = &rest[success_pos + 8..];
            if after_success.is_empty() || after_success.chars().all(|c| c.is_whitespace()) {
                let program = &rest[..success_pos];
                return LogEntry::Success { _program: program.to_string() };
            }
        }

        // Program <pubkey> failed: <error>
        if let Some(failed_pos) = rest.find(" failed: ") {
            let program = &rest[..failed_pos];
            let error = &rest[failed_pos + 9..];
            return LogEntry::Failed { _program: program.to_string(), error: error.to_string() };
        }

        // Program log: <message>
        if let Some(log_msg) = rest.strip_prefix("log: ") {
            return LogEntry::Log { message: log_msg.to_string() };
        }

        // Program data: <base64>
        if let Some(data) = rest.strip_prefix("data: ") {
            return LogEntry::Data { data: data.to_string() };
        }

        // Program return: <pubkey> <data>
        if let Some(return_rest) = rest.strip_prefix("return: ") {
            if let Some(space_pos) = return_rest.find(' ') {
                let program = &return_rest[..space_pos];
                let data = &return_rest[space_pos + 1..];
                return LogEntry::Return { _program: program.to_string(), data: data.to_string() };
            }
        }
    }

    LogEntry::Other(trimmed.to_string())
}

/// A structured log tree that groups logs by instruction.
#[derive(Debug)]
pub struct InstructionLogs {
    /// The instruction index (0-based)
    pub instruction_index: usize,
    /// The program ID for this instruction
    pub program: String,
    /// Log entries for this instruction (including nested CPI logs)
    pub entries: Vec<LogEntryWithDepth>,
}

/// A log entry with its depth in the CPI tree.
#[derive(Debug, Clone)]
pub struct LogEntryWithDepth {
    /// The CPI depth (1 = top-level, 2 = first CPI, etc.)
    pub depth: u32,
    /// The actual log entry
    pub entry: LogEntry,
}

/// Parse all logs and group them by instruction index.
///
/// This function tracks program invocations to associate logs with their
/// corresponding top-level instructions.
pub fn parse_logs_by_instruction(logs: &[String]) -> Vec<InstructionLogs> {
    let mut result: Vec<InstructionLogs> = Vec::new();
    let mut current_instruction: Option<usize> = None;
    let mut depth_stack: Vec<String> = Vec::new(); // Stack of program IDs
    let mut instruction_counter: usize = 0;

    for line in logs {
        let entry = parse_log_line(line);

        match &entry {
            LogEntry::Invoke { program, depth } => {
                let d = *depth as usize;

                // Adjust stack to match current depth
                while depth_stack.len() >= d {
                    depth_stack.pop();
                }
                depth_stack.push(program.clone());

                if *depth == 1 {
                    // New top-level instruction
                    current_instruction = Some(instruction_counter);
                    result.push(InstructionLogs {
                        instruction_index: instruction_counter,
                        program: program.clone(),
                        entries: Vec::new(),
                    });
                    instruction_counter += 1;
                }

                // Add invoke entry to current instruction
                if let Some(idx) = current_instruction {
                    if let Some(inst_logs) = result.get_mut(idx) {
                        inst_logs.entries.push(LogEntryWithDepth { depth: *depth, entry });
                    }
                }
            }
            LogEntry::Success { _program: _ } | LogEntry::Failed { _program: _, error: _ } => {
                // Add to current instruction before popping
                if let Some(idx) = current_instruction {
                    if let Some(inst_logs) = result.get_mut(idx) {
                        let depth = depth_stack.len() as u32;
                        inst_logs.entries.push(LogEntryWithDepth { depth, entry });
                    }
                }
                depth_stack.pop();
            }
            _ => {
                // Add other entries to current instruction
                if let Some(idx) = current_instruction {
                    if let Some(inst_logs) = result.get_mut(idx) {
                        let depth = depth_stack.len() as u32;
                        inst_logs.entries.push(LogEntryWithDepth { depth, entry });
                    }
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_invoke() {
        let line = "Program 11111111111111111111111111111111 invoke [1]";
        match parse_log_line(line) {
            LogEntry::Invoke { program, depth } => {
                assert_eq!(program, "11111111111111111111111111111111");
                assert_eq!(depth, 1);
            }
            _ => panic!("Expected Invoke"),
        }
    }

    #[test]
    fn test_parse_invoke_depth_2() {
        let line = "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [2]";
        match parse_log_line(line) {
            LogEntry::Invoke { program, depth } => {
                assert_eq!(program, "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
                assert_eq!(depth, 2);
            }
            _ => panic!("Expected Invoke"),
        }
    }

    #[test]
    fn test_parse_log() {
        let line = "Program log: Instruction: Transfer";
        match parse_log_line(line) {
            LogEntry::Log { message } => {
                assert_eq!(message, "Instruction: Transfer");
            }
            _ => panic!("Expected Log"),
        }
    }

    #[test]
    fn test_parse_consumed() {
        let line = "Program 11111111111111111111111111111111 consumed 150 of 200000 compute units";
        match parse_log_line(line) {
            LogEntry::Consumed { program, used, total } => {
                assert_eq!(program, "11111111111111111111111111111111");
                assert_eq!(used, 150);
                assert_eq!(total, 200000);
            }
            _ => panic!("Expected Consumed"),
        }
    }

    #[test]
    fn test_parse_success() {
        let line = "Program 11111111111111111111111111111111 success";
        match parse_log_line(line) {
            LogEntry::Success { program } => {
                assert_eq!(program, "11111111111111111111111111111111");
            }
            _ => panic!("Expected Success"),
        }
    }

    #[test]
    fn test_parse_failed() {
        let line = "Program ABC123 failed: custom program error: 0x1";
        match parse_log_line(line) {
            LogEntry::Failed { program, error } => {
                assert_eq!(program, "ABC123");
                assert_eq!(error, "custom program error: 0x1");
            }
            _ => panic!("Expected Failed"),
        }
    }

    #[test]
    fn test_parse_data() {
        let line = "Program data: SGVsbG8gV29ybGQ=";
        match parse_log_line(line) {
            LogEntry::Data { data } => {
                assert_eq!(data, "SGVsbG8gV29ybGQ=");
            }
            _ => panic!("Expected Data"),
        }
    }

    #[test]
    fn test_parse_return() {
        let line = "Program return: ABC123 SGVsbG8=";
        match parse_log_line(line) {
            LogEntry::Return { program, data } => {
                assert_eq!(program, "ABC123");
                assert_eq!(data, "SGVsbG8=");
            }
            _ => panic!("Expected Return"),
        }
    }

    #[test]
    fn test_parse_other() {
        let line = "Some random log message";
        match parse_log_line(line) {
            LogEntry::Other(msg) => {
                assert_eq!(msg, "Some random log message");
            }
            _ => panic!("Expected Other"),
        }
    }

    #[test]
    fn test_parse_logs_by_instruction() {
        let logs = vec![
            "Program 11111111111111111111111111111111 invoke [1]".to_string(),
            "Program log: Test message".to_string(),
            "Program 11111111111111111111111111111111 consumed 100 of 200000 compute units"
                .to_string(),
            "Program 11111111111111111111111111111111 success".to_string(),
            "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]".to_string(),
            "Program log: Transfer".to_string(),
            "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 500 of 200000 compute units".to_string(),
            "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success".to_string(),
        ];

        let result = parse_logs_by_instruction(&logs);
        assert_eq!(result.len(), 2);

        assert_eq!(result[0].instruction_index, 0);
        assert_eq!(result[0].program, "11111111111111111111111111111111");
        assert_eq!(result[0].entries.len(), 4);

        assert_eq!(result[1].instruction_index, 1);
        assert_eq!(result[1].program, "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
        assert_eq!(result[1].entries.len(), 4);
    }

    #[test]
    fn test_parse_nested_cpi() {
        let logs = vec![
            "Program ABC invoke [1]".to_string(),
            "Program log: Outer instruction".to_string(),
            "Program DEF invoke [2]".to_string(),
            "Program log: Inner instruction".to_string(),
            "Program DEF consumed 100 of 200000 compute units".to_string(),
            "Program DEF success".to_string(),
            "Program ABC consumed 500 of 200000 compute units".to_string(),
            "Program ABC success".to_string(),
        ];

        let result = parse_logs_by_instruction(&logs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].instruction_index, 0);
        assert_eq!(result[0].program, "ABC");
        assert_eq!(result[0].entries.len(), 8);

        // Check depths
        assert_eq!(result[0].entries[0].depth, 1); // ABC invoke
        assert_eq!(result[0].entries[1].depth, 1); // Outer log
        assert_eq!(result[0].entries[2].depth, 2); // DEF invoke
        assert_eq!(result[0].entries[3].depth, 2); // Inner log
        assert_eq!(result[0].entries[4].depth, 2); // DEF consumed
        assert_eq!(result[0].entries[5].depth, 2); // DEF success
        assert_eq!(result[0].entries[6].depth, 1); // ABC consumed
        assert_eq!(result[0].entries[7].depth, 1); // ABC success
    }
}
