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

## 画布平移的手感

当前有两处本地补丁（细节见 `vendor/patches/README.md`）：

- **受控平移**：渲染器原本给画布传 `infinite: ""`，而 `resolveProp` 语义是
  `props ?? 配置 ?? 缺省`——空串不回落到缺省，`Boolean("")` 为 false，画布实际走原生
  `scrollLeft`。原生滚动带系统阻尼/加速曲线（慢速移动位移很小），且 ui-frame 对
  `pointerType === "touch"` 在非无限模式下直接不启动拖拽——触屏拖不动。改成布尔 `true`
  后是逐事件 1:1 的 `translate(panX, panY)`。
- **滚轮倍率**：stage 自己做 `onWheel`（冒泡到 viewport 之前拦下），按 1.5 倍调
  `panBy`；`Ctrl`/`Meta` 放行给缩放，`textarea`/`input`/`.pd-doc-view` 放行给原生滚动。

渲染器的视口矩形（虚拟化裁剪用）读的是 `getView().panX/panY`，非无限模式下这两个恒为 0，
所以「受控平移」这条同时修掉了大文档群（> 40 框）滚到别处后框被裁掉的隐患。

## 组框会误伤时降级

组区域的几何 = 成员包围盒 + 内边距，而它是布局**之后**才算的——布局求解器
（`computeLayeredLayout(boxes, relations)`）根本不知道有组。因此「只框住成员」不是不变量：
跨 ≥2 层的组，其 y 区间必然覆盖中间整条层带。

渲染器按「不说假话」处理：检测到组区域里有**非成员框的中心点**时，不画虚线矩形，降级为
组名 chip（折叠入口保留、尺寸手柄隐藏）。判据用中心点而不是矩形相交——内边距只有
24/34px，相交会把紧贴的一圈邻居全算成侵入，几乎每组都要降级。折叠态不降级（成员不渲染，
画布上不存在交错）。

作者侧的对策是 `check` 的 `group-scattered` 规则，见 [文档图检查](./check.md)；格式规范
（技能 `prodoc-format` §4.5）把判据写成「组只用在『一个父 + 全部直接子』上」。

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

`npm test` 里有一组 `tests/vendor-patches.test.ts`：它读 `vendor/` 的产物文本、逐条断言
三份补丁还在。升级 vendor 覆盖 dist 后这三条会直接红——否则失效方式是静默的（画布照旧能
打开，只是行为退回上游默认）。

## 事件消费不能看长度

`state.moduleEvents` 是**所有模块共用**的 200 条滚动缓冲：填满之后 `length` 恒为 200。
早先按 `watch(() => state.moduleEvents.length)` 消费，一旦填满就再也不触发——新快照永远
不应用，换目录后画布卡在「正在扫描文档目录…」。现在看**头元素引用**（新事件总是
`unshift` 到头部），并补一次首跑但不据此恢复上次的根。
