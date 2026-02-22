# 重构：账户缓存机制 (`--cache`)

用 `--cache` 替换 `--dump-accounts` / `--load-accounts` / `--offline`，简化模拟重放工作流。

## 设计概要

- 移除 `--dump-accounts <DIR>`、`--load-accounts <DIR>`、`--offline`
- 新增 `--cache`、`--cache-dir <DIR>`、`--refresh-cache`
- 缓存存储在 `~/.sonar/cache/<KEY>/`，每个账户一个 `<PUBKEY>.json`
- `_meta.json` 标记缓存完整性：存在时完全离线（即使 config 里配了 `rpc_url` 也不走网络）
- Bundle 使用 `bundle-<SHA256_HEX>` 作为缓存键
- 新增 `sonar cache list/clean/info` 子命令

## 流程图

```
sonar simulate TX --cache
  ├─ --refresh-cache? ──Yes──→ 从 RPC 加载 → 写缓存 + _meta.json → 模拟
  └─ No
      ├─ _meta.json 存在? ──Yes──→ 完全离线加载 → 模拟
      └─ No ──→ 有 rpc_url? ──Yes──→ 从 RPC 加载 → 写缓存 + _meta.json → 模拟
                             └─ No ──→ 错误: 无缓存且无 RPC
```

---

## TODO List

### Phase 1: CLI 参数与配置

- [ ] **1.1** `src/cli/simulate.rs` — 移除 `dump_accounts`、`load_accounts`、`offline` 三个字段
- [ ] **1.2** `src/cli/simulate.rs` — 新增 `cache: bool`（`--cache`, env `SONAR_CACHE`）
- [ ] **1.3** `src/cli/simulate.rs` — 新增 `cache_dir: Option<PathBuf>`（`--cache-dir`, env `SONAR_CACHE_DIR`, requires `cache`）
- [ ] **1.4** `src/cli/simulate.rs` — 新增 `refresh_cache: bool`（`--refresh-cache`, requires `cache`）
- [ ] **1.5** `src/utils/config.rs` — `SonarConfig` 新增 `cache: Option<bool>` 和 `cache_dir: Option<String>`
- [ ] **1.6** `src/utils/config.rs` — `ConfigKey` 枚举新增 `Cache`、`CacheDir`，补充 `apply_config_to_env`
- [ ] **1.7** `src/cli/simulate.rs` — 移除 `parse_account_json` 函数中与 `--load-accounts` 相关的公开导出（如果不再需要外部调用）

### Phase 2: 缓存键与目录解析

- [ ] **2.1** 新增 `src/core/cache.rs` 模块（或在 `src/handlers/simulate.rs` 内）
- [ ] **2.2** 实现 `resolve_cache_dir(cache_dir: &Option<PathBuf>) -> PathBuf`
  - 优先 `cache_dir` 参数 → `SONAR_CACHE_DIR` 环境变量 → 默认 `~/.sonar/cache`
- [ ] **2.3** 实现 `derive_cache_key_single(input: &str, tx: &VersionedTransaction) -> String`
  - 输入是 signature → 用 signature
  - 输入是 raw bytes → 用 `tx.signatures[0]`
  - signature 全零 → 用 message SHA256 前 16 字节 hex
- [ ] **2.4** 实现 `derive_cache_key_bundle(inputs: &[String], txs: &[ParsedTransaction]) -> String`
  - 按顺序拼接各 single key 后取 SHA256，加 `bundle-` 前缀
- [ ] **2.5** 实现 `resolve_cache_state(cache, cache_dir, refresh_cache, cache_key) -> (Option<PathBuf>, bool)`
  - 返回 `(缓存目录, offline)`，offline 由 `_meta.json` 存在性 + `!refresh_cache` 决定
- [ ] **2.6** 添加 `sha2` crate 到 `Cargo.toml`（如尚未依赖）

### Phase 3: `_meta.json` 元数据

- [ ] **3.1** 定义 `CacheMeta` 结构体（Serialize/Deserialize）
  ```rust
  struct CacheMeta {
      created_at: String,       // RFC3339
      sonar_version: String,
      cache_type: String,       // "single" | "bundle"
      inputs: Vec<String>,      // 原始输入列表
      rpc_url: String,
      account_count: usize,
  }
  ```
- [ ] **3.2** 实现 `write_meta_json(dir: &Path, meta: &CacheMeta) -> Result<()>`
- [ ] **3.3** 实现 `read_meta_json(dir: &Path) -> Result<CacheMeta>`（供 `sonar cache info` 使用）

### Phase 4: AccountLoader 改造

- [ ] **4.1** `src/core/account_loader.rs` — `AccountLoader` 新增 `cache_write_dir: Option<PathBuf>` 字段
- [ ] **4.2** `AccountLoader::new` 签名改为接收 `local_dir`, `cache_write_dir`, `offline`（移除原来对 `--load-accounts` 路径的直接依赖）
- [ ] **4.3** `fetch_accounts` — 在 Layer 4（RPC 获取）完成后，如果 `cache_write_dir` 有值，将新获取的账户写入缓存目录
  - 复用 `executor::write_dump_account` 或提取为共享函数
  - 同时写入零 lamport placeholder（与当前 `dump_accounts_to_dir` 逻辑一致）
- [ ] **4.4** `fetch_accounts` — offline 模式的警告信息从 `"--load-accounts directory"` 改为 `"cache directory"`
- [ ] **4.5** `AccountLoader::new` — offline 模式下允许 rpc_url 为空的逻辑保持不变

### Phase 5: Handler 改造

- [ ] **5.1** `src/handlers/simulate.rs` — `handle` 函数：移除 `dump_accounts`, `load_accounts`, `offline` 参数使用
- [ ] **5.2** `handle` 函数：新增缓存初始化逻辑
  - 调用 `derive_cache_key_single` + `resolve_cache_state`
  - 构造 `AccountLoader` 使用 `tx_cache_dir` 和 `offline`
  - 账户加载完成后（非 offline 时）调用 `dump_accounts_to_dir` + `write_meta_json`
- [ ] **5.3** `handle` 函数：IDL auto-fetch 判断从 `!offline`（CLI flag）改为 `!offline`（由 `_meta.json` 推导）— 逻辑代码不变，但语义来源变了
- [ ] **5.4** `handle_bundle` 函数：同上改造
  - 使用 `derive_cache_key_bundle` 计算缓存键
  - 使用 `resolve_cache_state` 确定 offline 状态
  - 账户加载完成后写 `_meta.json`（bundle 类型，包含所有 inputs）
- [ ] **5.5** `SimulateArgs` 解构处：替换 `dump_accounts, load_accounts, offline` 为 `cache, cache_dir, refresh_cache`
- [ ] **5.6** `handle_bundle` 参数列表：替换 `dump_accounts, load_accounts, offline` 为 `cache, cache_dir, refresh_cache`

### Phase 6: `sonar cache` 子命令

- [ ] **6.1** `src/cli/mod.rs` — `Command` 枚举新增 `Cache(CacheArgs)` variant
- [ ] **6.2** 新增 `src/cli/cache.rs` — 定义 `CacheArgs` 及子命令
  ```
  sonar cache list                    # 列出所有缓存
  sonar cache clean [--older-than Xd] # 清理缓存
  sonar cache info <KEY>              # 显示缓存详情
  ```
- [ ] **6.3** 新增 `src/handlers/cache.rs` — 实现 `handle(args: CacheArgs)`
  - `list`: 遍历缓存根目录，读取各 `_meta.json`，格式化输出（signature/bundle, 账户数, 创建时间）
  - `clean`: 按时间过滤并删除目录
  - `info`: 读取指定缓存的 `_meta.json`，显示详情 + 账户文件列表
- [ ] **6.4** `src/main.rs` — 在 command dispatch 中添加 `Command::Cache` 分支

### Phase 7: 清理旧代码

- [ ] **7.1** `src/core/executor.rs` — `dump_accounts_to_dir` 改为 `pub(crate)`（仅内部使用，不再由 CLI 直接调用）
- [ ] **7.2** `src/cli/simulate.rs` — 如果 `parse_account_json` 仅供 `AccountLoader` 使用，移入 `account_loader.rs` 或 `cache.rs`
- [ ] **7.3** 移除 `HELP_HEADING_STATE_PREPARATION` 下与旧选项相关的注释/文档

### Phase 8: 测试

- [ ] **8.1** `tests/e2e_cli_output_streams.rs` — 更新 `offline_missing_account_does_not_trigger_strict_offline_error` 测试，改用 `--cache` 语义
- [ ] **8.2** 新增测试：`--cache` 首次运行创建缓存目录 + `_meta.json`
- [ ] **8.3** 新增测试：`--cache` 二次运行从缓存离线加载（不触发 RPC）
- [ ] **8.4** 新增测试：`--refresh-cache` 忽略已有缓存
- [ ] **8.5** 新增测试：bundle `--cache` 创建 `bundle-<hash>` 目录
- [ ] **8.6** 新增测试：`sonar cache list` / `sonar cache clean` 基本功能
- [ ] **8.7** 新增单元测试：`derive_cache_key_single` 各种输入格式
- [ ] **8.8** 新增单元测试：`derive_cache_key_bundle` 顺序敏感性

### Phase 9: 文档与质量

- [ ] **9.1** 更新 `README.md` — 替换 `--dump-accounts` / `--load-accounts` / `--offline` 相关说明为 `--cache` 用法
- [ ] **9.2** 更新 `AGENTS.md` — Common Commands 部分更新示例
- [ ] **9.3** 运行 `cargo fmt --check` 确认格式
- [ ] **9.4** 运行 `cargo clippy -- -D warnings` 确认无警告
- [ ] **9.5** 运行 `cargo test` 确认所有测试通过
- [ ] **9.6** 手动验证：`sonar simulate <SIG> --cache` 首次 + 二次重放流程

---

## 实现顺序建议

Phase 1-2 → Phase 3 → Phase 4 → Phase 5 → Phase 7 → Phase 8 → Phase 6 → Phase 9

原因：核心缓存功能（Phase 1-5, 7）先实现并通过测试，`sonar cache` 子命令（Phase 6）是独立功能可以后做。

## 涉及文件清单

| 文件 | 操作 |
|------|------|
| `src/cli/simulate.rs` | 修改（CLI 参数替换） |
| `src/cli/mod.rs` | 修改（新增 Cache 子命令） |
| `src/cli/cache.rs` | 新增 |
| `src/utils/config.rs` | 修改（新增 cache/cache_dir 配置） |
| `src/core/cache.rs` | 新增（缓存键/目录/元数据逻辑） |
| `src/core/account_loader.rs` | 修改（新增 cache_write_dir） |
| `src/core/executor.rs` | 小改（可见性调整） |
| `src/handlers/simulate.rs` | 修改（使用新缓存逻辑） |
| `src/handlers/cache.rs` | 新增 |
| `src/handlers/mod.rs` | 修改（导出 cache handler） |
| `src/main.rs` | 修改（新增 Cache dispatch） |
| `Cargo.toml` | 可能修改（sha2 依赖） |
| `tests/e2e_cli_output_streams.rs` | 修改 |
| `README.md` | 修改 |
| `AGENTS.md` | 修改 |
