---
prodoc: true
id: panel-ui
title: 面板 UI
group: 交付
---

# 面板 UI

`panel-ui/`：一个入口动作（「文档」）+ 一个 overlay——选文档根 → 拉快照 → 交给 ProDoc 的
`DocGraphViewer` 渲染成文档图画布。

## 入口与懒加载

画布组件走 `defineAsyncComponent`：它把渲染器（布局 / 边路由 / 搜索 / 编辑器）一起带进来，
约 240 KB。面板首屏只需要一个「文档」按钮，没必要为没打开过文档群的人付这份体积。

扩展向 Core 登记（`lib::register_panel_ui`），Panel 启动时经 `echo-agent.panel-ui.list`
拿到清单再 `import()`。此前只能靠 `PANEL_UI_EXTENSIONS` 在面板构建期编进产物——改一行 UI
要重建整个面板。

## 数据流

```text
扩展 sendCommand(RequestProDocBundle)
       └─► Core 命令通道 ─► prodoc 模块扫描目录
                                  │
扩展读 state.moduleEvents ◄───────┘  ModuleEvent{prodoc.bundle}
```

三条读命令（`RequestProDocBundle` / `BrowseProDocDirs` / `ProDocRoots`）的输出**全部走模块
事件**：宿主 `apply_command` 只认 `Err`，`Ok(value)` 被丢弃。这条如果搞错，表现是「命令发了、
界面永远转圈」，而且日志一片安静。

## 为什么按 core+team+root 分键

面板能同时连多个 Core（多上游聚合），同一 Core 下多个 team 也可能配了同名目录。只按 `root`
分键会让它们互相覆盖——表现是「切了上游、画布还是上一个 Core 的文档」，极难从界面上看出来。
所以事件缓存键是三元组。

## 写盘与合并

保存走 `write.ts` 的 `saveFile`：带 `base` 提交 → 冲突时用 vendored 的 `merge3` 自动合并 →
以 `base = 磁盘当前内容` 重发。

**合并刻意留在前端**：CLI 用的就是 `prodoc-core/merge.ts` 这份 diff3，在 Rust 复刻必然语义
分叉（「面板说合并成功、命令行说冲突」）。后端只做「逐字比对 + 原子写」，见
[写盘与冲突](./write.md)。

`merge3` 是动态 `import()`：只有真出现「磁盘被外部改过」时才加载那 6 KB。

## vendored 目录

| 目录 | 内容 | 来源 |
|---|---|---|
| `vendor/prodoc-renderer/` | `index.js` + `index.css` + 精简 `index.d.ts` | `DocRenderer/packages/prodoc-renderer/dist` |
| `vendor/prodoc-core/` | `pure.js` + 依赖 chunk + 精简 `pure.d.ts` | `DocRenderer/packages/prodoc-core/dist`（只用 `merge3`） |

`vendor/` 是构建产物，给它们打的补丁记在 `vendor/patches/`（每处含症状、原因与升级时如何
重放）。**升级覆盖 dist 之前先读那份 README**，否则补丁会静默丢失。

## 构建与测试

在 `panel-ui/` 目录里自己构建与测试（扩展不再是面板构建的一部分，运行期由 Panel 按
Core 登记的路径 `import()`）：

```bash
cd panel-ui
npx vite build   # 固定产物 dist/index.js + 带 hash 的 chunk
npm test         # vitest，配置见 panel-ui/vitest.config.ts（纯函数层，不需要 DOM）
```

`panel-extension.json` 只声明 `id` 与入口（`index.ts`）；产物落在 `panel-ui/dist/`，由部署
脚本复制到 Panel 静态目录的 `panel-extensions/echo-agent.prodoc/`。

## 事件消费不能看长度

`state.moduleEvents` 是**所有模块共用**的 200 条滚动缓冲：填满之后 `length` 恒为 200。
早先按 `watch(() => state.moduleEvents.length)` 消费，一旦填满就再也不触发——新快照永远
不应用，换目录后画布卡在「正在扫描文档目录…」。现在看**头元素引用**（新事件总是
`unshift` 到头部），并补一次首跑但不据此恢复上次的根。
