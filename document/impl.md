---
prodoc: true
id: impl
title: 代码结构
group: 实现
link: ["scan | 扫描与上限", "write | 写盘与冲突", "check | 文档图检查"]
---

# 代码结构

一个 crate（`crates/agent-module-prodoc`），六个源文件，边界按「谁最容易出错」划。

## 文件分工

| 文件 | 职责 | 关键约束 |
|---|---|---|
| `lib.rs` | 模块装配、状态、事件出口、清单、技能与面板 UI 登记 | 身份写在 `module.toml`（单一真值） |
| `commands.rs` | 七个命令的解析与回执翻译 | 只翻译，不做判断 |
| `bundle.rs` | 目录扫描 → `相对路径 → 原文` | 语义对齐 CLI 的 `loadMarkdownFiles` |
| `check.rs` | 文档图检查（参数区解析 + 图算法） | 复刻 CLI 的**图结构**子集 |
| `write.rs` | 单文件原子写 / 删 / 读，冲突判定 | 逐字比对，与 CLI 的 `write-api.ts` 对齐 |
| `paths.rs` | 相对路径 → 根内真实路径 | 读写共用的唯一入口 |

## 分层

```prodoc-flow
graph TB
  Cmd[commands.rs] --> Scan[bundle.rs]
  Cmd --> Chk[check.rs]
  Cmd --> Wr[write.rs]
  Wr --> Pth[paths.rs]
  Scan --> Pth
```

- `paths.rs` 是**唯一**路径入口。读走一条、写走另一条是这类模块最常见的洞，所以读（`ReadProDocFile`）
  与写共用 `paths::resolve`，等写盘上线时路径校验已经是被测过的。
- `bundle.rs` / `check.rs` / `write.rs` 是纯逻辑（入参出参，状态不进），单测直接调；三者的
  单元测试都落在各自文件尾。
- `commands.rs` 只做「解析载荷 → 调纯逻辑 → 发事件 → 组回执」。判定留在纯逻辑里，是为了
  一眼看出哪条是「CLI 也这么做」、哪条是本模块额外加的。
- `lib.rs` 把上面的东西接到框架上：`BuiltinModule::new(manifest, mount)` +
  `.with_commands` + `.with_command_handler_async`。

## 装配流程

1. `module(version)` → `module_with_state(version)`：构造 `Arc<ModuleState>`，克隆两份进闭包。
2. 挂载闭包：解析 `ctx.config()` → `set_config`；有事件出口则 `set_event_sink`。
3. `register_skill(ctx)`：把 `SKILL.md` 交给技能注册接缝，拿 `Disposer`（卸载时撤销）。
4. `register_panel_ui(ctx)`：把前端 bundle 的站点相对路径登记给面板 UI 注册表。
5. 两个 `Disposer` 一起返回给宿主，模块被 `enabled = false` 时界面与提示词一起消失。

任一步失败都只是**降级**：接缝缺席 → warn 一条、返回空 `Vec`；配置解析失败 → 按缺省继续。
文档能力本身不依赖这两者。

## 状态与缓存

`ModuleState` 只有两项：配置（`RwLock<Config>`）与事件出口（`RwLock<Option<EventSink>>`）。

**不缓存文件内容**。文档群是无状态的：每次请求都从磁盘重新读——目录可能被 `git checkout`
切分支、被别的编辑器改。缓存会让「磁盘变了但面板没刷新」变成常态。

## 清单与身份

```toml
# crates/agent-module-prodoc/module.toml
id = "echo-agent.prodoc"
name = "ProDoc 文档群"
kind = "tool"
builtin = true
tag = "inner"
optional_requires = ["echo-agent.host.skill_registrar"]
```

`include_str!` 读入并在编译期固化：清单散在部署目录里会让「同一个二进制在不同机器上表现
不同」。版本号由宿主注入（`manifest(version)`），crate 内不写死。

`optional_requires` 有两个作用：技能的软依赖（缺它时 ProDoc 照常工作），以及宿主的服务依赖
拓扑排序（技能模块先挂载、接缝先就位）。

## 事件出口

所有事件收口在 `ModuleState::emit_*`：`emit_bundle` / `emit_roots` / `emit_dirs` /
`emit_write` / `emit_report` / `emit_error`。命令里不直接拼 `ModuleEvent`——事件名集中在
`lib::events` 的常量区，见 [事件协议](./events.md)。
