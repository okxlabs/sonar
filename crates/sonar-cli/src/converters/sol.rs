/// 1 SOL = 1_000_000_000 lamports.
pub(crate) const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
const LAMPORTS_PER_SOL_U128: u128 = 1_000_000_000;

pub(crate) fn parse_sol_to_lamports(input: &str) -> Result<u64, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("SOL amount cannot be empty".to_string());
    }
    if input.starts_with('-') {
        return Err("SOL amount cannot be negative".to_string());
    }
    if input.starts_with('+') {
        return Err("SOL amount must not use '+' sign".to_string());
    }

    let parts: Vec<&str> = input.split('.').collect();
    if parts.len() > 2 {
        return Err(format!("Invalid SOL value '{}'", input));
    }

    let int_part_raw = parts[0];
    let frac_part_raw = if parts.len() == 2 { parts[1] } else { "" };

    if !int_part_raw.is_empty() && !int_part_raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid SOL value '{}'", input));
    }
    if !frac_part_raw.is_empty() && !frac_part_raw.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("Invalid SOL value '{}'", input));
    }
    if parts.len() == 2 && int_part_raw.is_empty() && frac_part_raw.is_empty() {
        return Err(format!("Invalid SOL value '{}'", input));
    }
    if frac_part_raw.len() > 9 {
        return Err("SOL supports up to 9 decimal places".to_string());
    }

    let int_part = if int_part_raw.is_empty() {
        0u128
    } else {
        int_part_raw
            .parse::<u128>()
            .map_err(|e| format!("Invalid SOL integer part '{}': {}", int_part_raw, e))?
    };

    let mut frac_scaled = 0u128;
    if !frac_part_raw.is_empty() {
        let frac_digits = frac_part_raw
            .parse::<u128>()
            .map_err(|e| format!("Invalid SOL fraction part '{}': {}", frac_part_raw, e))?;
        let scale = 10u128.pow(9 - frac_part_raw.len() as u32);
        frac_scaled = frac_digits * scale;
    }

    let lamports = int_part
        .checked_mul(LAMPORTS_PER_SOL_U128)
        .and_then(|v| v.checked_add(frac_scaled))
        .ok_or_else(|| "SOL value overflows lamports range".to_string())?;

    u64::try_from(lamports).map_err(|_| "SOL value overflows u64 lamports".to_string())
}

pub(crate) fn format_sol(lamports: u64) -> String {
    if lamports == 0 {
        return "0".to_string();
    }

    let integer = lamports / LAMPORTS_PER_SOL;
    let fraction = lamports % LAMPORTS_PER_SOL;
    if fraction == 0 {
        return integer.to_string();
    }

    let mut frac_str = format!("{:09}", fraction);
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    format!("{}.{}", integer, frac_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_whole_sol() {
        assert_eq!(parse_sol_to_lamports("1").unwrap(), 1_000_000_000);
    }

    #[test]
    fn parse_fractional_sol() {
        assert_eq!(parse_sol_to_lamports(".5").unwrap(), 500_000_000);
    }

    #[test]
    fn parse_full_precision() {
        assert_eq!(parse_sol_to_lamports("1.23456789").unwrap(), 1_234_567_890);
    }

    #[test]
    fn parse_smallest_unit() {
        assert_eq!(parse_sol_to_lamports("0.000000001").unwrap(), 1);
    }

    #[test]
    fn rejects_too_many_decimals() {
        let err = parse_sol_to_lamports("1.0000000001").unwrap_err();
        assert!(err.contains("up to 9"));
    }

    #[test]
    fn rejects_negative() {
        assert!(parse_sol_to_lamports("-1").is_err());
    }

    #[test]
    fn rejects_plus_sign() {
        assert!(parse_sol_to_lamports("+1").is_err());
    }

    #[test]
    fn rejects_bare_dot() {
        assert!(parse_sol_to_lamports(".").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_sol_to_lamports("").is_err());
    }

    #[test]
    fn format_zero() {
        assert_eq!(format_sol(0), "0");
    }

    #[test]
    fn format_whole() {
        assert_eq!(format_sol(1_000_000_000), "1");
    }

    #[test]
    fn format_fractional() {
        assert_eq!(format_sol(1_500_000_000), "1.5");
    }

    #[test]
    fn format_full_precision() {
        assert_eq!(format_sol(1_234_567_890), "1.23456789");
    }

    #[test]
    fn format_trailing_zeros_trimmed() {
        assert_eq!(format_sol(1_100_000_000), "1.1");
    }

    #[test]
    fn format_single_lamport() {
        assert_eq!(format_sol(1), "0.000000001");
    }
}
