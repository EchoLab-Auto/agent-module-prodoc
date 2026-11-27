---
prodoc: true
id: deliver
title: 交付形态
group: 交付
link: ["panel-ui | 面板 UI", "host | 宿主接线", "dev | 开发与测试"]
---

# 交付形态

本仓交付三样东西，缺一样功能就不完整。

| 交付物 | 位置 | 消费方 |
|---|---|---|
| 模块 crate | `crates/agent-module-prodoc` | Core（内置模块，`tag = "inner"`） |
| 面板 UI bundle | `panel-ui/` | Panel（运行期 `import()`） |
| 格式技能 | `skills/prodoc-format/SKILL.md` | 技能模块 → 提示词 |

## 上游依赖

```prodoc-flow
graph LR
  Core[EchoAgentCore] --> Crate[agent-module-prodoc]
  Crate --> Skill[prodoc-format 技能]
  Crate --> Registry[面板 UI 注册表]
  Registry --> Panel[Panel]
  Crate --> Evt[模块事件]
  Evt --> Panel
```

Core 只保留**通用接线**：依赖、内置模块登记、配置、工具转发壳。ProDoc 的扫描、路径沙箱、
写盘、检查全在本仓。

## 模块清单是单一真值

身份写在 `module.toml`（`id` / `name` / `kind` / `builtin` / `tag` /
`optional_requires`），`lib.rs` 用 `include_str!` 在编译期读入。改标题或描述不必改 Rust。

`id` 是运行期标识（配置、面板门控、持久化状态都用它），**不随 crate 名变**。

## 技能随模块走

`prodoc-format` 讲的是 ProDoc 的文档图模型（目录即图、文件即框、参数区、`link`、`group`）。
渲染与解析都由本 crate 实现，格式规范自然归它：放在 Core 的技能目录里，规范改了却忘了同步
Core 就是一次静默的误导。

登记挪到模块自己身上还解决了另一个问题：以前归属写在 Core 的 `skill_contributions()` 里，
模块被 `enabled = false` 关掉时技能照样进提示词——模型被教了一套本部署没有的命令。现在技能
的有无与挂载状态是同一件事，卸载时由 `Disposer` 撤销。

只交正文、不解析：`SKILL.md` 的 frontmatter 规则属于技能模块（接缝在 `echo-defs`），本
crate 因此不必依赖解析器。接缝缺席只是降级（`optional_requires` 是软依赖）。

## 面板 UI 贡献

`register_panel_ui` 把前端 bundle 的站点相对路径登记给 Core：

```rust
pub const PANEL_UI_BUNDLE: &str = "panel-extensions/echo-agent.prodoc/index.js";
pub const PANEL_UI_SDK_VERSION: &str = "1";
```

固定路径不带 hash：面板运行期靠它 `import()`，名字一变就失联。渲染器是预构建产物，会额外
产出若干带 hash 的 chunk——那些由 `index.js` 相对引用，跟它一起部署即可。

注册表缺席（嵌入式部署没有宿主）是降级：文档能力仍可用，缺的只是界面入口。

## 三条验收线

| 层 | 怎么验 |
|---|---|
| crate | `cargo test` / `cargo clippy --all-targets -- -D warnings` |
| 接线 | 面板「文档」入口能看到本仓 `document/`；打开后画布有 12 个框 |
| 格式 | `prodoc_check` 或 `echo-prodoc view ./document` 无 error 级问题 |

具体命令见 [开发与测试](./dev.md)。
