# vendored 产物的本地补丁

`vendor/` 下两个目录是上游 `DocRenderer` 仓的**构建产物**（不是源码，也不是 npm
依赖），所以给它们打补丁时改的是压缩后的 dist。这里登记每一处补丁，避免升级
vendor 时静默丢失。

升级流程（见 `../../README.md`）覆盖 `index.js` / `index.css` 之前，先确认下面
每一条在新产物里仍然需要；需要就把对应改动重放到新产物上。

## `2026-11-relayout-save.patch` — 分层重排的落盘出口

**症状**：点「🧭 分层重排」后画布变了，但没有任何按钮能把这批坐标写回文件。

**原因**：上游里 `relayouted`（坐标覆盖表）是**纯视图变换**——源码注释写得很
明确：「仅当前视图，不写回文件」。而「💾 保存」只遍历编辑模式的草稿
`pendingDrafts`，重排不碰草稿，所以两条路都不通。

**补丁内容**（`vendor/prodoc-renderer/index.js`）：

1. 新增三个状态：`layoutSavedFlag` / `layoutSavingFlag` / `layoutDoneFlag`
   （后者是前两者的组合 computed，用于模板文案与禁用态）。
2. `setPositionOverride`（产物里叫 `dr`）复位 `layoutSavedFlag`——任何手改坐标
   都让"已保存"失效。
3. `toggleRelayout`（`mr`）在已落盘时短路，不再"恢复坐标"；重排开始时复位标记。
4. 新增 `saveRelayout`：遍历覆盖表，对 `docPath` 非空、且坐标确实与文件不同的
   框调 `writeFramePosition` 写回参数区，逐篇 `requestSave`。
   - 有未保存正文草稿的路径**跳过**：那里只能用磁盘原文当基准，拿它写坐标会把
     草稿里的正文改动一并冲掉。
   - 失败的文件保留在覆盖表里（画布仍是用户看到的样子），只 toast。
5. 模板：重排按钮右侧新增「💾 保存布局」，仅在覆盖表非空且未落盘时出现。

**为什么不直接全量替换成上游新构建**：本机对比过 `DocRenderer` main 的
`dist/index.js`，它**缺**「折叠分组」与「搜索文档」两处代码（105928B vs 64034B），
说明我们 vendored 的是更晚的版本。全量替换会丢掉这两个功能。

**已知残留**：保存成功后不立即清空覆盖表。Core 的 prodoc 模块在
`SaveProDocFile` 之后只回单文件回执，**不重推 bundle**（CLI 的本地 server 会推
热更新，渲染器假定有这个行为）。清空覆盖表会让画布立刻跳回旧坐标，看起来像
保存失败，所以保留覆盖表、用「✓ 布局已保存」表达状态；下一次重开文档群或刷新
页面时，坐标从文件读出，覆盖表由 `watch(() => props.files)` 自动修剪。
