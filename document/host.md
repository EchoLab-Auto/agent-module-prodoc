---
prodoc: true
id: host
title: 宿主接线
group: 交付
---

# 宿主接线

本模块是 Core 的**内置模块**（`tag = "inner"`，以 git 依赖参与 Core 构建）。下面五步是
Core 侧需要维护的全部内容。

## 1. 依赖

```toml
# EchoAgentCore 的 Cargo.toml
agent-module-prodoc = { git = "https://github.com/EchoLab-Auto/agent-module-prodoc", branch = "main" }
```

框架类型（`echo-module`）按 tag 取自 `echo-fabric-arch`，必须与 Core 根 `Cargo.toml` 保持同一
tag——同一进程里不能出现两份 `Box<dyn Module>`。

## 2. 内置模块登记

```rust
// source/modules/core/src/builtin_modules.rs
push_module(&mut catalog, agent_module_prodoc::module(version))?;
```

## 3. 门控清单

`scripts/update.sh` 的 `expected_ids` 加 `echo-agent.prodoc`。它与 `BUILTIN_MODULE_IDS` 的
一致性由 `instance_manifests` 测试守护——漏加的表现是「模块装了但被当作未知 id」。

## 4. 配置

```toml
[[module]]
id = "echo-agent.prodoc"
kind = "builtin"
name = "ProDoc 文档群"
enabled = true

[module.config]
# 空 = 只允许 HOME。不配这一项等于给面板开了一个「读全盘 .md」的口子。
roots = ["/root/work/agent-module-prodoc"]
```

`roots` 的语义与沙箱细节见 [配置与沙箱](./config.md)。

## 5. 工具转发壳

宿主侧的工具 `prodoc_check` 是**转发壳**（`agent-module-loop` 的 `agent/prodoc_tool.rs`）：
它按 `agent_module_prodoc::commands::CHECK_COMMAND` 分发，工具名与命令名共用常量，避免两处
字面量漂移。回执用 `check::CheckReceipt`——宿主读的是同一个类型，字段改名会两侧一起编译
失败，而不是让工具静默报告「0 个错误」。

## 面板扩展

```bash
PANEL_UI_EXTENSIONS="/path/to/agent-module-workspace/panel-ui:/path/to/agent-module-prodoc/panel-ui" \
  bash ~/.local/libexec/echo-agent-panel/update.sh --source /path/to/EchoAgentPanel
```

装好后 `~/.local/state/echo-agent-panel/ui-extensions` 会记住这份清单，后续更新自动沿用。
2026-11 起面板也可以在运行期从 Core 的动态注册表拿到 bundle（`echo-agent.panel-ui.list`），
不再强依赖构建期清单——两条路都指向 `panel-extensions/echo-agent.prodoc/index.js`。

## 部署后自检

1. `modules.toml` 里 `enabled = true`，启动日志有 `prodoc 模块已挂载`（带 roots 数与上限）
2. 面板「文档」入口列出预期文档群（`prodoc.roots` 事件）
3. 打开本仓 `document/` 能看到 12 个框；`prodoc_check` 无 error
4. 未声明 `prodoc: true` 的目录被拒绝，且错误文案里带字段名
