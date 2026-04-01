# Bug Report: Claude 条目去重导致费用高估

## 概要

TokenMonitor 对 Claude Code JSONL 条目的去重逻辑存在缺陷，导致同一 API 调用的 token 被重复计算。用户报告 TokenMonitor 显示的费用约为 AWS Bedrock 实际账单的 **2.34 倍**。

## 用户报告的数据

| 来源 | 费用 | 说明 |
|------|------|------|
| AWS Bedrock 控制台 | **$303.18** | Claude Opus 4.6 (Bedrock Edition) 实际账单 |
| TokenMonitor | **$709.11** | 同一时间段，Opus 4.6 |
| TokenMonitor 显示 token | **416.9M** | |
| 比率 | **2.34x** | TokenMonitor / Bedrock |

注：Bedrock 和 Anthropic 直连的标价完全一致（Opus 4.6: input=$5/M, output=$25/M, cache_write=$6.25-$10/M, cache_read=$0.50/M），因此不是定价差异问题。

## 根因分析

### Bug #1: 去重 hash 包含 `isSidechain` 和 `agentId`，导致同一 API 响应生成不同的 hash

**文件:** `src-tauri/src/usage/parser.rs`，`create_claude_unique_hash` 函数（~第 375 行）

```rust
fn create_claude_unique_hash(entry: &ClaudeJsonlEntry) -> Option<String> {
    let message_id = entry.message.as_ref()?.id.as_ref()?;
    let request_id = entry.request_id.as_ref()?;
    let sidechain = if entry.is_sidechain == Some(true) { "1" } else { "0" };  // ← BUG
    let agent = entry.agent_id.as_deref().unwrap_or("");                        // ← BUG
    Some(format!("{sidechain}:{agent}:{message_id}:{request_id}"))
}
```

**问题：** Claude Code 在 JSONL 中对同一个 API 响应（相同 `message.id` + `requestId`）会写入多条记录。这些记录除了 `isSidechain` 和 `agentId` 字段不同外，其他字段（包括所有 token 计数）完全相同。当前的 hash 函数将这些字段纳入计算，导致同一个 API 调用被视为不同条目。

**实际数据证据：**

```
msg_01XHC3ZC...:req_011CYszQ... → 16 条记录：
  - isSidechain: true,  agentId: "acompact-a4450356be4f87e8"  → hash A
  - isSidechain: false, agentId: undefined                     → hash B
  → 同一 API 调用被计费 2 次！
```

**影响量化（全时段 Opus 4.6 数据）：**

| 去重策略 | 条目数 | Token | 费用 |
|----------|--------|-------|------|
| 不去重 | 24,747 | 2,243.3M | $2,309.00 |
| 当前 hash（含 sidechain+agent） | 11,579 | 1,139.0M | $954.19 |
| **仅用 message_id:requestId** | **9,550** | **867.1M** | **$758.10** |

- 多计条目：2,029（+21.2%）
- 多计费用：$196.09（+25.9%）
- 受影响的唯一 API 调用数：1,420（占所有重复组的 21.2%）

### Bug #2: 去重保留第一条（流式中间状态）而非最后一条（最终结果）

**文件：** `src-tauri/src/usage/parser.rs`，`load_integration_entries_with_debug` 函数（~第 1872 行）

```rust
if !processed_hashes.insert(unique_hash.clone()) {
    continue;  // 跳过后续条目 → 保留第一条（stop_reason: null，output 不完整）
}
```

**问题：** Claude Code 的流式响应会产生多条记录：

```
#1: stop_reason=null,     output_tokens=28   ← 去重保留这条！
#2: stop_reason=null,     output_tokens=28
#3: stop_reason=null,     output_tokens=28
#4: stop_reason=tool_use, output_tokens=320  ← 这才是最终值
```

去重保留了第一条（`output_tokens=28`），丢弃了最终条（`output_tokens=320`）。对于 output 来说这是**低估**，但方向与 Bug #1 相反。

注意：input_tokens、cache_creation、cache_read 在所有流式条目中相同，只有 output_tokens 不同。

## 修复建议

### Bug #1 修复

将 hash 函数改为仅使用 `message_id` 和 `request_id`：

```rust
fn create_claude_unique_hash(entry: &ClaudeJsonlEntry) -> Option<String> {
    let message_id = entry.message.as_ref()?.id.as_ref()?;
    let request_id = entry.request_id.as_ref()?;
    Some(format!("{message_id}:{request_id}"))
}
```

**注意：** 如果需要保留 `agent_scope` 标注（用于 subagent 统计），应在去重后设置，取条目组中的 sidechain 值。或者保留第一条的 scope 即可。

### Bug #2 修复

在去重时保留**最后一条**条目（具有最终 output_tokens），而不是第一条。两种方案：

**方案 A：** 在文件解析阶段只保留每个 hash 的最后一条：

```rust
// parse_claude_session_file 中，使用 HashMap 存储每个 hash 的最新条目
let mut entry_map: HashMap<String, ParsedEntry> = HashMap::new();
// ... 遍历时用 insert 覆盖旧值 ...
```

**方案 B：** 在 `load_integration_entries_with_debug` 中改为后来者覆盖：

```rust
// 第一遍：收集所有条目，按 hash 分组，保留最后一条
// 第二遍：推入结果
```

推荐方案 A，因为它在文件解析阶段就减少了数据量，减轻后续处理的负担。

## 复现步骤

1. 使用 Claude Code 进行一次包含多轮对话的会话
2. 确保 Claude Code 产生了 subagent 调用（如使用 Agent 工具）
3. 在 TokenMonitor 中查看费用
4. 对比 AWS Bedrock 控制台的实际账单
5. 观察到 TokenMonitor 显示费用约为实际的 1.3x-2.3x

## 测试验证

现有测试 `parse_claude_dedupes_null_stop_reason_entries_by_message_and_request` 只覆盖了相同 `isSidechain`/`agentId` 的情况。需要添加：

```rust
#[test]
fn parse_claude_dedupes_entries_with_different_sidechain_flags() {
    // 同一 message_id + requestId，但 isSidechain 不同
    // 期望：只保留一条
}

#[test]
fn parse_claude_dedupes_keeps_last_entry_with_final_output() {
    // 同一 hash 的多条记录，stop_reason 从 null 到 end_turn
    // 期望：保留 stop_reason 非 null 的最后一条
}
```

## 优先级

**高** — 直接导致费用显示错误，影响用户对 token 使用量的判断。
