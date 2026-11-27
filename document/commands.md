---
prodoc: true
id: commands
title: 命令契约
group: 接口契约
link: ["events | 事件协议", "config | 配置与沙箱"]
---

# 命令契约

模块声明七个命令（`commands::COMMANDS`）。宿主按名分发到
`commands::handle`，返回 `None` 表示「不认领」（不是错误）。

## 七个命令

| 命令 | 载荷（除 `team_id`） | 回执 | 事件 |
|---|---|---|---|
| `RequestProDocBundle` | `root` | `{root, documents, bytes, truncated}` | `prodoc.bundle` |
| `BrowseProDocDirs` | `path`（可空） | `{path, entries}` 或 `{roots, entries:0}` | `prodoc.dirs` |
| `ProDocRoots` | — | `{roots, configured, require_marker}` | `prodoc.roots` |
| `ReadProDocFile` | `root`, `path`, `request_id` | `{ok}` | `prodoc.file` |
| `SaveProDocFile` | `root`, `path`, `content`, `base`?, `request_id` | `{ok}` | `prodoc.saved` |
| `DeleteProDocFile` | `root`, `path`, `base`?, `request_id` | `{ok}` | `prodoc.deleted` |
| `ProDocCheck` | `root` | 完整 [检查报告](./check.md) | `prodoc.report` |

`team_id` 可选，原样带回事件载荷——面板能同时连多个 Core，靠它过滤。

## 回执为什么这么薄

宿主 `apply_command` 只认 `Err`（转成错误事件），`Ok(value)` 的返回值被丢弃。所以凡是有
**体量**的输出（快照、文件原文、检查报告）一律走模块事件；回执只留几项统计，方便日志与
工具代理读。

唯一例外是 `ProDocCheck`：它的回执带**完整报告**（`CheckReceipt`）。面板不看回执，而工具
代理（宿主的 `prodoc_check`）拿到的正是回执——它要能逐条念给模型听。两条出口共用同一份
数据，不额外存状态。

## 三条命令的读取语义

- `RequestProDocBundle`：`root` 必填，必须是配置发现范围（含子目录）内的真实路径；否则发
  `prodoc.error` 并 `Err`。目录自己没声明 `prodoc: true` 时同样拒绝（`require_marker`）。
- `BrowseProDocDirs`：`path` 为空 = 返回**发现的文档群**（不是配置原样），供选择器兜底；
  有值时只列**子目录**，不列文件——选的是文档根，不是文件。
- `ProDocRoots`：返回的是「哪些目录真的声明了自己」，不是「配置里写了哪些路径」。

## 写盘三命令的共同前置

三者共用 `write_target`：解析 `root` / `path` / `request_id`，先做根沙箱校验
（canonicalize 后前缀比较），再进写逻辑。路径本身的校验在 `paths::resolve`，见
[写盘与冲突](./write.md)。

`request_id` 由面板生成并原样带回。事件没有天然关联键（同一文件可以连续存两次），靠
`(root, path)` 匹配会在快速连点时错配。

## 阻塞命令在阻塞池里跑

扫描、写盘、检查都是纯 CPU + 文件 IO，一个几千篇的文档群能占住命令通道几百毫秒。三条命令
全部 `tokio::task::spawn_blocking`：文档目录可能在网络盘上，一次 `rename` 就能卡住整条命令
通道，进而连累别的模块。

## 命令名是常量

宿主侧的工具 `prodoc_check` 按 `commands::CHECK_COMMAND` 分发，模块内也用它。工具名与
命令名共用常量，避免两处字面量漂移——漂移的表现是「工具存在但永远说模块未启用」。

## 常见误用

| 现象 | 原因 |
|---|---|
| 命令成功但界面转圈 | 只读了回执，没订阅模块事件 |
| `root` 永远打不开 | 配置 `roots` 里没有它，或目录没有 `prodoc: true` |
| 保存一直冲突 | 提交时没带 `base`（缺省 = 无条件覆盖，不会冲突），或 `base` 不是磁盘当前内容 |
| 面板显示两个同名按钮 | 两个根末段同名；`label` 会差异化，见 [配置与沙箱](./config.md) |
