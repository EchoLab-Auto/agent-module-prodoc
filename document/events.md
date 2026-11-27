---
prodoc: true
id: events
title: 事件协议
group: 接口契约
---

# 事件协议

模块通过 `ModuleState::emit_event` 发事件，宿主不认识事件名，于是原样包成
`BackendEvent::ModuleEvent` 推给面板；扩展在 `state.moduleEvents` 里按名字取。这条路是
**现成的**——不需要给线协议加变体，也不必改 Panel 的类型定义。

## 八条事件

| 事件 | 载荷 | 触发 |
|---|---|---|
| `prodoc.bundle` | `{team_id, root, files, skipped, stats}` | `RequestProDocBundle` |
| `prodoc.error` | `{team_id, root, error}` | 任一命令失败 |
| `prodoc.roots` | `{roots, caps}` | `ProDocRoots` |
| `prodoc.dirs` | `{path, roots, entries}` | `BrowseProDocDirs` |
| `prodoc.file` | `WriteAck` | `ReadProDocFile` |
| `prodoc.saved` | `WriteAck` | `SaveProDocFile` |
| `prodoc.deleted` | `WriteAck` | `DeleteProDocFile` |
| `prodoc.report` | `{team_id, root, stats, issues, ok}` | `ProDocCheck` |

`files` 是 `相对路径 → 完整原文`（**含**框架参数区），`stats` 含 `documents` / `bytes` /
`skipped` / `truncated`。`skipped` 是被跳过的条目及原因（超限、非 UTF-8、读失败），
见 [扫描与上限](./scan.md)。

## WriteAck 的形状

读取 / 保存 / 删除共用一份结构（`lib::WriteAck`）：三种操作的判定分支高度重叠（内容比对 →
冲突时附磁盘内容），共用一份便于「改造一处、三处同时正确」。

| 字段 | 含义 |
|---|---|
| `request_id` | 面板生成的关联 id，原样带回 |
| `team_id` | 归属人格 |
| `root` / `path` | 文档根（绝对）与相对路径 |
| `ok` | 操作是否成功 |
| `error` | 失败原因（人话，直接显示） |
| `conflict` | 需要前端合并：磁盘内容既不是 `base` 也不是本次提交内容 |
| `disk` | 磁盘当前内容；`None` = 文件不存在（冲突原因之一是「已被删除」） |
| `content` | 读取结果（仅 `prodoc.file`） |

## 为什么内容不走回执

宿主命令分发只认 `Err`：`Ok(value)` 的返回值被丢弃。把快照塞进回执的后果不是「慢」，是
**面板永远收不到**。这也是 Panel UI 扩展里最容易踩的一脚——`sendCommand` 的返回值不能当
数据用。

## 检查报告不复用 error

`prodoc.error` 在面板上是「这次操作失败了」的红条；`prodoc.report` 的 `ok: false` 只表示
文档图里有 error 级问题——文档本来就允许有未完成的草稿。两者混用会让「检查有断链」看起来
像「检查命令失败」。

## 事件出口为什么可缺

`emit` 槽位是 `Option<EventSink>`，`mount` 时注入一次。嵌入部署或单测里没有 sink 时
`emit_event` 静默丢弃而不是 panic——裸装配下模块仍需可构造、可测试。

## 面板侧取用

```text
扩展 sendCommand(RequestProDocBundle)
       └─► Core 命令通道 ─► prodoc 模块扫描目录
                                  │
扩展读 state.moduleEvents ◄───────┘  ModuleEvent{prodoc.bundle}
```

事件按 `core + team + root` 分键缓存，细节见 [面板 UI](./panel-ui.md)。
