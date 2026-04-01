# Configuration

## Config File

Sonar reads configuration from `~/.config/sonar/config.toml`:

```toml
rpc_url = "https://api.mainnet-beta.solana.com"
idl_dir = "~/.sonar/idls"

# Default for `simulate/decode --no-idl-fetch`
no_idl_fetch = false
# Default for `simulate --show-balance-change`
show_balance_change = false
# Default for `simulate --show-ix-detail`
show_ix_detail = false
# Default for `simulate --raw-log`
raw_log = false
# Default for `simulate/decode --raw-ix-data`
raw_ix_data = false
# Default for `simulate --check-sig`
verify_signatures = false
# Default for `send --skip-preflight`
skip_preflight = false
# Default for `simulate --cache`
cache = false
# Default for `simulate --cache-dir`
cache_dir = "~/.sonar/cache"
```

Priority: CLI arguments > environment variables > config file > defaults.

## Config Command

View or modify `~/.config/sonar/config.toml`:

```bash
# List all supported config items
sonar config list

# Get one config value
sonar config get show_ix_detail

# Set one config value
sonar config set show_ix_detail=true

# Alternative assignment form
sonar config set show_ix_detail true
```

## Cache Command

Manage cached account data for offline simulation:

```bash
sonar cache list
sonar cache clean --older-than 7d
sonar cache info <KEY>
```

## Completions

Generate shell completion scripts:

```bash
sonar completions bash > ~/.local/share/bash-completion/completions/sonar
sonar completions zsh > ~/.zsh/completions/_sonar
sonar completions fish > ~/.config/fish/completions/sonar.fish
```
