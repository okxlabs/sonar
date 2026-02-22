# Sonar AI Fix Brief (Issues 1,2,3,4,6,7,8)

本文档面向代码修改型 AI，目标是把以下问题改成"行为一致、可脚本化、文档可信"的状态。

## Scope

只覆盖以下问题编号:

- [x] 1: `simulate/decode` 的 stdin 支持与 CLI 必填参数冲突
- [x] 2: `--offline` 实际行为与帮助文案冲突
- [x] 3: `decode` bundle 模式下 `--json` 输出不是单一合法 JSON 文档
- [x] 4: `idl fetch/sync` 在部分失败时仍返回 0
- [x] 6: `send` 的 explorer 链接未根据网络集群区分
- [x] 7: stdout/stderr 约定与部分子命令行为不一致
- [x] 8: `cnofig` typo alias 被公开暴露并写入 README

---

## Issue 1 - stdin support is blocked by required TX arg

### Severity

P0

### Problem

README 声明支持从 stdin 读取交易输入，但 `TX` 被 clap 定义为 required，导致无 positional 参数时命令在参数解析阶段直接失败，根本进不到 handler 的 stdin fallback 逻辑。

### Evidence

- `src/cli/simulate.rs:130` (`tx: Vec<String>` + `required = true`)
- `src/handlers/simulate.rs:100` (single tx path tries fallback stdin)
- `src/handlers/decode.rs:32` (single tx path tries fallback stdin)
- `README.md:82` (`cat ./transaction.txt | sonar simulate --rpc-url <RPC_URL>`)

### Repro

```bash
echo "<TX>" | sonar simulate --rpc-url https://api.mainnet-beta.solana.com
echo "<TX>" | sonar decode --rpc-url https://api.mainnet-beta.solana.com
```

现状: clap 报缺少 `TX`。  
预期: 命令应成功从 stdin 读取。

### Recommended Change

1. 让 `TransactionInputArgs.tx` 在 CLI 层可为空。
2. 保留现有 `read_raw_transaction(tx_single)` 逻辑作为统一入口。
3. 明确优先级:
   - 有 positional `TX` -> 用 positional
   - 无 positional 且 stdin 非 TTY -> 读 stdin
   - 两者都无 -> 返回友好错误

### Acceptance Criteria

- `simulate`/`decode` 无 `TX` 且 stdin 有内容时可工作。
- 无 `TX` 且无 stdin 时错误信息清晰。
- bundle 模式(`tx.len() > 1`)不回归。

### Tests to Add/Adjust

- e2e: `simulate/decode` omitted `TX` + stdin success。
- e2e: omitted `TX` + empty stdin fail with actionable message。

---

## Issue 2 - offline semantics mismatch (`--offline`)

### Severity

P0

### Problem

帮助文案写的是缺失账号会报错，但实现是告警后继续，把缺失账号当作"不存在账号"处理，语义上属于宽松模式。

### Evidence

- `src/cli/simulate.rs:74` (`--offline` 描述: error if missing)
- `src/core/account_loader.rs:358` (offline path warns and returns `Ok(())`)

### Repro

```bash
sonar simulate <TX> --load-accounts ./partial_dump --offline
```

当 dump 不完整时:

- 现状: 仅 stderr warning，继续执行。
- 文案预期: 直接错误退出。

### Recommended Change

二选一，需统一产品语义:

1. 推荐方案: 改为严格模式  
   `--offline` 下只要存在非 native/sysvar 缺失账号就返回错误。
2. 备选方案: 保持宽松实现  
   明确重写 help/README，说明 `--offline` 仅禁用 RPC，缺失账号将被视为不存在并继续。

### Acceptance Criteria

- CLI 描述、README、运行时行为三者一致。
- 用户能明确知道是否发生了"不完整重放"。

### Tests to Add/Adjust

- 单测/集成测试覆盖 offline 缺失账号行为(严格失败或宽松继续，取决于最终定案)。

---

## Issue 3 - bundle decode JSON output is not a single valid JSON document

### Severity

P1

### Problem

`decode` 在 bundle + `--json` 时逐个 `println!(pretty_json)`，输出是多个 JSON 对象拼接，既不是 JSON array，也不是显式 NDJSON 契约，不利于脚本消费。

### Evidence

- `src/handlers/decode.rs:131` (loop render each parsed tx)
- `src/output/mod.rs:90` (`render_transaction_only` prints one pretty JSON object)

### Repro

```bash
sonar decode <TX1> <TX2> --json --rpc-url <RPC_URL> | jq .
```

现状: `jq` 通常报错或需特殊处理多对象流。  
预期: 默认输出单一合法 JSON 文档。

### Recommended Change

推荐默认行为:

- bundle + `--json` 输出 `[{...}, {...}]`。

如果保留流式输出能力，可新增显式开关:

- `--json-lines` 输出 NDJSON，每行一个对象。

### Acceptance Criteria

- bundle + `--json` 可直接被 `jq` 解析。
- 文档说明普通 JSON 与 JSONL 的差异(若引入 `--json-lines`)。

### Tests to Add/Adjust

- e2e: bundle decode json should parse as one valid JSON array。

---

## Issue 4 - `idl fetch/sync` returns exit 0 on partial failure

### Severity

P1

### Problem

实现会把 `not found`/`error` 打印到 `stderr`，但最终仍 `Ok(())`，对 CI/脚本意味着"成功"，无法据退出码自动判断是否完整成功。

### Evidence

- `src/handlers/idl.rs:89` (print `no IDL found`)
- `src/handlers/idl.rs:93` (print fetch error)
- `src/handlers/idl.rs:96` (still `Ok(())`)

### Repro

```bash
sonar idl fetch <VALID_PROGRAM> <INVALID_PROGRAM> --rpc-url <RPC_URL>
echo $?
```

现状: 常见部分失败情况下仍为 `0`。  
预期: 可配置或默认在部分失败时非 0。

### Recommended Change

建议行为:

1. 默认严格: 只要有 `errors` 或 `not_found` 即返回 `Err(...)`。
2. 若有兼容顾虑，新增 `--allow-partial`，仅在该模式下保持 exit 0。
3. stdout 保持只输出成功写入的文件路径，失败统计写 stderr。

### Acceptance Criteria

- 无法完整 fetch/sync 时退出码可被脚本可靠感知。
- 输出中包含成功/失败计数摘要。

### Tests to Add/Adjust

- e2e: partial failure should return non-zero (or gated by `--allow-partial`)。

---

## Issue 6 - explorer URL ignores cluster

### Severity

P2

### Problem

`send` 里 explorer 链接固定 mainnet 样式，对 devnet/testnet 用户会跳到错误网络页面。

### Evidence

- `src/handlers/send.rs:48` (fixed `https://explorer.solana.com/tx/{signature}`)

### Repro

```bash
sonar send <SIGNED_TX> --rpc-url https://api.devnet.solana.com
```

现状: 链接无 cluster 参数。  
预期: devnet/testnet 自动附加对应 cluster 参数。

### Recommended Change

1. 依据 `rpc_url` 推断 cluster:
   - devnet -> `?cluster=devnet`
   - testnet -> `?cluster=testnet`
   - 其他默认 mainnet(不加参数)或按策略处理
2. 无法可靠推断时，至少不要误导，考虑输出:
   - signature(稳定主输出)
   - explorer URL(尽力推断)
   - stderr 提示 "verify cluster manually"

### Acceptance Criteria

- devnet/testnet 常见 RPC 域名能生成正确 explorer 集群链接。
- 现有 mainnet 行为不回归。

### Tests to Add/Adjust

- 单测覆盖 URL 构造: mainnet/devnet/testnet。

---

## Issue 7 - stdout/stderr convention is not consistently enforced

### Severity

P2

### Problem

README 约定"主结果 stdout，警告/错误 stderr"，但实现中存在成功信息进入 stderr 的情况，导致自动化消费需要额外特判。

### Evidence

- Convention: `README.md:61`
- `send --wait` 将确认信息放 stderr: `src/handlers/send.rs:35`
- `program-elf -o <file>` 成功写入提示放 stderr: `src/handlers/program_elf.rs:80`

### Recommended Change

先定统一规则，再改实现:

1. 规则建议:
   - stdout: 机器可消费的主结果(结构化/稳定字段)
   - stderr: warning、diagnostic、error
2. 按规则重排:
   - `send --wait` 的确认状态可并入 stdout(或提供 `--quiet/--verbose` 区分)
   - `program-elf` 成功写入提示可转 stdout，或在 README 明确其为诊断信息
3. 对脚本关键命令补充稳定输出契约说明。

### Acceptance Criteria

- 文档约定与实现一致。
- 常见 shell pipeline 不需要猜哪个流有成功信息。

### Tests to Add/Adjust

- e2e 输出流测试: success path 下 stdout/stderr 分布符合约定。

---

## Issue 8 - typo alias `cnofig` is publicly exposed

### Severity

P3

### Problem

`config` 子命令公开了 typo alias `cnofig`，且 README 直接示例使用，降低产品专业性并制造歧义。

### Evidence

- `src/cli/mod.rs:97` (`alias = "cnofig"`)
- `README.md:326` (`sonar cnofig list`)

### Recommended Change

推荐分阶段处理:

1. 立即从 README 移除 typo 示例。
2. 兼容层保留 1 个版本周期:
   - 仍可解析 `cnofig`
   - stderr 打印 deprecation 警告，提示改用 `config`
3. 下个 major 移除 alias。

如果不需要兼容，可直接删除 alias 并同步测试。

### Acceptance Criteria

- 用户文档只出现 `config`。
- 是否保留兼容 alias 的策略清晰并有迁移提示。

### Tests to Add/Adjust

- 若保留兼容: 增加 deprecated 行为测试。
- 若删除兼容: 删除/更新 `cnofig` 相关测试。

---

## Suggested Execution Order (for AI)

为减少回归，建议按顺序改:

1. Issue 1 (stdin contract)  
2. Issue 2 (offline contract)  
3. Issue 3 (bundle JSON contract)  
4. Issue 4 (exit code contract)  
5. Issue 7 (stream contract)  
6. Issue 6 (explorer cluster)  
7. Issue 8 (alias cleanup)

每一步都同步更新:

- CLI help 文案
- README 示例
- e2e/单测
- 输出流与退出码行为说明
