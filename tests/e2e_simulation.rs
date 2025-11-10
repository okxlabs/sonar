use assert_cmd::Command;

const V0_RAW_TX: &str = "GPdrKqMbYtzsysuEJhYG4bpUB9xQFpdQ8ps9s8XorbfD5SA5FrFfMAL2oznLNP9Ah4wXPe6Y9BVkAXM7Gw47whMxuK5TCKvpKtkyDEiuYfaRZCmv1mk5u16HvPzQqXGHzmf3iFUraHA2yEghbqaJsUW27PmXWvs2xPhK1WtFBvF4PNxtFBNa7sGwHZPmT88zk5pwpVnseAu48HhDvY6Nj7qjTzRAAFczubznScT4aT1m5CNyYjVwYjR5iqc7PrpTzyAxevb1Zk1ndXgHfwnQAhZfKfV712i5z352Jbf96WdQFGva3f22NGSVWtSFjp6agEBDTvWVUa3Db4WvArURczERDymqEEhX5EfMSZTUYenfgRXL2kgjWoXkuFaDyumgapdyqzQFixL4aJZCEDp6yfq7V5g2WYqwqNXHBsKbfpTfqKsqCV1niunXSZfGTTRgXjFWXuQNbtLrbd9TTJmUhsTMJuPzhohT89yX292vmUDGvHv3YqkJGynKcEGT6cPrB6ayWiBGybsJ2fUiax7QFPKT1hscSm5HDJPV3HmrC3DyHQAWq6hrPzGMeUcEBfSEtkvPFtNe9kpw4N9x2bJwuiGRgHbVmzDnRGMdXpu9KWigYb3uebLTFLUDeDq1CfD377AzdxBkBJQkdQ3peTjAz1kW3pEQQLiE57p16Wf8oUgxCveHpGr73RCoveDsjeF7puENGkM2aFkmKLBRvW3yJHL9mCP6ZkuScMy9VCWkh554yEs72DEZU25Upj7RAAhc7zG7iWyP6m2gZg6gGZ5hqjrCasQXUjJkaPT4LgBeLS9W5scn7QA51QMZi95DgAvD9mQeSFixnFofpqNDTNWVnisoQQ2eAEPVwKC1sfKjBdMgrMJKG1JjzDM7DWRzR7xPYSyjPfHXHx5aZJ8LdyYjYXjS6dViihtH3sNebZzqLdDzERDEe6bAFAkB5tGcRdqF1kcdPN3HNeRgeA7xvJg5r3kaDkAQQ9yjZV2stexZ1eDa4KiRwBY3MsgujEntBT992CrU4uAtHKkSXUbusXHMjbx9Dn57HD9GEdzAnDq53Gmmz7xU2qKZ3hKhLZg3ZtYVTeAysqUSfZTRPp87VjdyrG2Msk32ufPdQbAZ2x9FYbUCPpRvLMPsNhDe5D4fMt9X63bQTsk9VQNx39j5Mo6NkYvcKmKiz3pb5J19bHnnSKixEKa89typqcbNFunudMMmAAT3egyok3WRCqwC83EwNBWwrdm5zs1mRefEDu77arj7E";

const LEGACY_RAW_TX: &str = "2fqzynyEVCxNh9c7B8H89AJeseMufVtKHiVxT5XYEU3tTb1Q3nj99XG62w69uWVb87vm6PHVvZJAdBw1LXAL9HChXtSuB5PZDkozebYGfYHw4ffSBb75igNgUPbf5R5vrRpsMzEgtr1vrhmPt4UgH6khq18n86bgVpENF5YXmrmMwjAjTNuLxDCzVKQ8YdNyepLioUS7igdjfLTDKus3dZst4w2TdhnqRLJmGaF7Ly4zdxbUVKKh5ekyf2WEy82Q2rwgoK5bpU1XaA3ZVFN5ADPUxVm36CzqbcL9hn91QVujETj3NXCU23CbdRMbESLnUFgq5XJepZbB6J9H9agmofBqtQy8ZcsiAnWD9sEDm71qeCzFJJBV712sErEAMdr1MEvrXDgmwufWSkNKsZp7Ez79VUqS4dckgK3k1W9JiN6VySzVVgQCPWxfxN6DZQSYFmxEWTBpqFR7C3arx8H9T3q226g6cZ8zMb7N9nCqAdBUakHLVYuDGsiyLGWwCgVdTNLbvzEtg47vNFtnTyuW9YD8SHFGjNkXjwh6bLk37d566r4vKRkXxm6581EVZXd1CtLpGXEAewb7fpDS9AfPUrLWU4irWFtHz2FW1Ka4Xi82oVub2Ka5gF8SirmoVnqPzyYPCJbMiEmd26UhdNamFhDwJv1BzmK5yeK95WwWB8CsBWyeywv8oExEqXgpwiKrx5Dfak9atjdwgxb3tkCSYTCfkx2MY3WuAfK8xdE242NoUmkoR2aVhmQhKeexfqFKAYz4zXMbykpYnF6cnbuN59oXqadnUFoi1WahQZNtBTgDWzEsAQEu9rw";

/// Requires mainnet RPC access, ignored by default. To run:
/// `cargo test --test e2e_simulation -- --ignored --nocapture`
#[test]
#[ignore]
fn simulate_real_transaction_via_cli() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("solsim")?;
    cmd.arg("simulate")
        .arg("--tx")
        .arg(V0_RAW_TX)
        .arg("--rpc-url")
        .arg("https://api.mainnet-beta.solana.com")
        .arg("--replace")
        .arg("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=tests/fixtures/spl_token.so")
        .arg("--output")
        .arg("text");

    let assert = cmd.assert().success();
    let output = assert.get_output();

    println!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        println!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[test]
#[ignore]
fn parse_real_transaction_via_cli() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("solsim")?;
    cmd.arg("simulate")
        .arg("--tx")
        .arg(V0_RAW_TX)
        .arg("--rpc-url")
        .arg("https://api.mainnet-beta.solana.com")
        .arg("--parse-only")
        .arg("--output")
        .arg("text");

    let assert = cmd.assert().success();
    let output = assert.get_output();

    println!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        println!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[test]
#[ignore]
fn simulate_real_transaction_replace_finalized_program() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("solsim")?;
    cmd.arg("simulate")
        .arg("--tx")
        .arg(V0_RAW_TX)
        .arg("--replace")
        .arg("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=tests/fixtures/spl_token.so")
        .arg("--output")
        .arg("text");

    let assert = cmd.assert().success();
    let output = assert.get_output();

    println!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        println!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[test]
#[ignore]
fn simulate_real_transaction_replace_upgradable_program() -> Result<(), Box<dyn std::error::Error>>
{
    let mut cmd = Command::cargo_bin("solsim")?;
    cmd.arg("simulate")
        .arg("--tx")
        .arg(V0_RAW_TX)
        .arg("--replace")
        .arg("proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u=tests/fixtures/dex_solana_v3.so")
        .arg("--output")
        .arg("text");

    let assert = cmd.assert().success();
    let output = assert.get_output();

    println!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        println!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

#[test]
#[ignore]
fn simulate_real_transaction_replace_multiple_program() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("solsim")?;
    cmd.arg("simulate")
        .arg("--tx")
        .arg(V0_RAW_TX)
        .arg("--replace")
        .arg("proVF4pMXVaYqmy4NjniPh4pqKNfMmsihgd4wdkCX3u=tests/fixtures/dex_solana_v3.so")
        .arg("--replace")
        .arg("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA=tests/fixtures/spl_token.so")
        .arg("--output")
        .arg("text");

    let assert = cmd.assert().success();
    let output = assert.get_output();

    println!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        println!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}
