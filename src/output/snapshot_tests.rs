//! Snapshot tests for terminal text rendering.
//!
//! These tests capture the exact styled output of key rendering functions
//! so layout regressions are caught automatically.  Run `cargo insta review`
//! after intentional visual changes to update the snapshots.

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::output::report::{SolBalanceChangeSection, TokenBalanceChangeSection};

    /// Capture helper: render into a buffer with colors disabled.
    fn capture<F: FnOnce(&mut Vec<u8>)>(f: F) -> String {
        colored::control::set_override(false);
        let mut buf = Vec::new();
        f(&mut buf);
        buf.flush().unwrap();
        String::from_utf8(buf).unwrap()
    }

    // -------------------------------------------------------------------
    // SOL balance change table
    // -------------------------------------------------------------------

    #[test]
    fn sol_balance_changes_single_positive() {
        let changes = vec![SolBalanceChangeSection {
            account: "So11111111111111111111111111111111111111112".into(),
            before: 1_000_000_000,
            after: 2_500_000_000,
            change: 1_500_000_000,
            change_sol: 1.5,
        }];
        let output = capture(|w| {
            crate::output::text::render_sol_balance_changes(&changes, "", None, w);
        });
        insta::assert_snapshot!("sol_balance_single_positive", output);
    }

    #[test]
    fn sol_balance_changes_mixed() {
        let changes = vec![
            SolBalanceChangeSection {
                account: "FeePayerABCDEFGH1234567890abcdefghijklm123".into(),
                before: 5_000_000_000,
                after: 4_990_000_000,
                change: -10_000_000,
                change_sol: -0.01,
            },
            SolBalanceChangeSection {
                account: "ReceiverXYZ9876543210zyxwvutsrqponmlkji987".into(),
                before: 0,
                after: 10_000_000,
                change: 10_000_000,
                change_sol: 0.01,
            },
        ];
        let output = capture(|w| {
            crate::output::text::render_sol_balance_changes(&changes, "", None, w);
        });
        insta::assert_snapshot!("sol_balance_mixed", output);
    }

    // -------------------------------------------------------------------
    // Token balance change table
    // -------------------------------------------------------------------

    #[test]
    fn token_balance_changes_single() {
        let changes = vec![TokenBalanceChangeSection {
            owner: "OwnerABCDEFGH123456789012345678901234567890".into(),
            token_account: "TokenAcctXYZ9876543210zyxwvutsrqponmlk1234".into(),
            mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
            before: 1_000_000,
            after: 500_000,
            change: -500_000,
            decimals: 6,
            ui_change: -0.5,
        }];
        let output = capture(|w| {
            crate::output::text::render_token_balance_changes(&changes, "", None, w);
        });
        insta::assert_snapshot!("token_balance_single", output);
    }

    #[test]
    fn token_balance_changes_multiple_tokens() {
        let changes = vec![
            TokenBalanceChangeSection {
                owner: "Owner111111111111111111111111111111111111111".into(),
                token_account: "ATA_USDC_111111111111111111111111111111111".into(),
                mint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v".into(),
                before: 100_000_000,
                after: 50_000_000,
                change: -50_000_000,
                decimals: 6,
                ui_change: -50.0,
            },
            TokenBalanceChangeSection {
                owner: "Owner111111111111111111111111111111111111111".into(),
                token_account: "ATA_SOL__111111111111111111111111111111111".into(),
                mint: "So11111111111111111111111111111111111111112".into(),
                before: 0,
                after: 1_000_000_000,
                change: 1_000_000_000,
                decimals: 9,
                ui_change: 1.0,
            },
        ];
        let output = capture(|w| {
            crate::output::text::render_token_balance_changes(&changes, "", None, w);
        });
        insta::assert_snapshot!("token_balance_multiple", output);
    }

    // -------------------------------------------------------------------
    // Structured log rendering
    // -------------------------------------------------------------------

    #[test]
    fn execution_trace_structured() {
        let logs = vec![
            "Program ComputeBudget111111111111111111111111111 invoke [1]".into(),
            "Program ComputeBudget111111111111111111111111111 consumed 300 of 200000 compute units"
                .into(),
            "Program ComputeBudget111111111111111111111111111 success".into(),
            "Program 11111111111111111111111111111111 invoke [1]".into(),
            "Program log: Transfer 1 SOL".into(),
            "Program 11111111111111111111111111111111 consumed 2100 of 199700 compute units".into(),
            "Program 11111111111111111111111111111111 success".into(),
        ];
        let output = capture(|w| {
            crate::output::text::render_logs_structured(&logs, w);
        });
        insta::assert_snapshot!("execution_trace_structured", output);
    }

    #[test]
    fn execution_trace_with_cpi() {
        let logs = vec![
            "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA invoke [1]".into(),
            "Program log: Instruction: Transfer".into(),
            "Program 11111111111111111111111111111111 invoke [2]".into(),
            "Program 11111111111111111111111111111111 consumed 500 of 180000 compute units".into(),
            "Program 11111111111111111111111111111111 success".into(),
            "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA consumed 4200 of 200000 compute units".into(),
            "Program TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA success".into(),
        ];
        let output = capture(|w| {
            crate::output::text::render_logs_structured(&logs, w);
        });
        insta::assert_snapshot!("execution_trace_cpi", output);
    }

    #[test]
    fn execution_trace_with_failure() {
        let logs = vec![
            "Program 11111111111111111111111111111111 invoke [1]".into(),
            "Program log: Attempting transfer".into(),
            "Program 11111111111111111111111111111111 failed: insufficient funds".into(),
        ];
        let output = capture(|w| {
            crate::output::text::render_logs_structured(&logs, w);
        });
        insta::assert_snapshot!("execution_trace_failure", output);
    }

    // -------------------------------------------------------------------
    // Section title
    // -------------------------------------------------------------------

    #[test]
    fn section_title_rendering() {
        let output = capture(|w| {
            crate::output::terminal::write_section_title(w, "SOL Balance Changes");
        });
        // Section titles depend on terminal width which varies; just check structure.
        assert!(output.starts_with('\n'));
        assert!(output.contains("SOL Balance Changes"));
        assert!(output.ends_with("\n\n"));
    }
}
