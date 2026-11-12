# ProDoc Panel UI 扩展

`echo-agent.prodoc` 的前端扩展：一个「文档」入口 + 一个 overlay，把 Core 扫出来的
文档群交给 ProDoc 的 `DocGraphViewer` 渲染成文档图画布。

## 安装

```bash
# Panel 安装/更新时带上本目录（可重复给多个扩展）
PANEL_UI_EXTENSIONS="/path/to/crate-workspace/panel-ui:/path/to/agent-module-prodoc/panel-ui" \
  bash ~/.local/libexec/echo-agent-panel/update.sh --source /path/to/EchoAgentPanel
```

装好后 `~/.local/state/echo-agent-panel/ui-extensions` 会记住这份清单，后续更新自动沿用。

## 构建与测试

从 Panel 的 `web/` 目录：

```bash
PANEL_UI_EXTENSIONS=/abs/path/to/agent-module-prodoc/panel-ui npm run build
PANEL_UI_EXTENSIONS=/abs/path/to/agent-module-prodoc/panel-ui npm test
```

## 数据流

```text
扩展 sendCommand(RequestProDocBundle)
       └─► Core 命令通道 ─► prodoc 模块扫描目录
                                  │
扩展读 state.moduleEvents ◄───────┘  ModuleEvent{prodoc.bundle} ─► DocGraphViewer
```

三条命令（`RequestProDocBundle` / `BrowseProDocDirs` / `ProDocRoots`）的**输出全部走模块
事件**：宿主 `apply_command` 只认 `Err`（转成错误事件），`Ok(value)` 被直接丢弃。这条
如果搞错，表现是"命令发了、界面永远转圈"，而且日志一片安静。

## 写盘与冲突合并

保存走 `write.ts` 的 `saveFile`：带 `base` 提交 → 冲突时用 vendored 的 `merge3`
自动合并 → 以 `base = 磁盘当前内容` 重发。**合并刻意留在前端**：CLI 用的就是
`prodoc-core/merge.ts` 这份 diff3，在 Rust 复刻必然语义分叉。

`merge3` 是动态 `import()`：只有真出现"磁盘被外部改过"时才加载那 6 KB。

## vendored 目录

| 目录 | 内容 | 来源 |
|---|---|---|
| `vendor/prodoc-renderer/` | `index.js` + `index.css` + 精简 `index.d.ts` | `DocRenderer/packages/prodoc-renderer/dist` |
| `vendor/prodoc-core/` | `pure.js` + 其依赖 chunk + 精简 `pure.d.ts` | `DocRenderer/packages/prodoc-core/dist`（只用 `merge3`） |

## vendored 渲染器

`vendor/prodoc-renderer/` 是 ProDoc 仓 `packages/prodoc-renderer/dist` 的产物
（`index.js` + `index.css` + 一份精简 `index.d.ts`），**不是** npm 依赖：

- 面板的扩展构建把外部扩展的裸包名 import 一律重定向到 `web/src/extension-api.ts`
  再解析（`extension-build.ts` 的 `resolveId`），所以 `import '@prodoc/renderer'` 在
  `web/node_modules` 里找不到就会失败；
- vendored 版本里的 `from '@echolab-auto/ui-frame'` / `from 'vue'` 会被解析到**面板自己
  那一份**——这正是我们要的：两份 ui-frame 会让 ThemeProvider 上下文与 CSS 变量打架。

升级步骤：在 ProDoc 仓 `npm run build`，把 `dist/index.js`、`dist/index.css` 覆盖过来，
删掉 `index.js` 末尾的 `//# sourceMappingURL=` 行（否则 vitest 会抱怨找不到 .map），
再跑一遍 `npm run build` 与 `npm test`。

## 它是**主区视图**，不是浮层（2026-11 起）

顶栏的「文档」与「聊天 / 设置 / Shell」同级，切换后独占主区。早先它是入口行的
一个浮层按钮 + overlay，实测不够用：

- 文档画布要独占主区（可平移缩放的画布塞在浮层里，边框与 padding 全是浪费）；
- "我在看哪个文档"属于"我在做什么"，该被顶栏选中态与 `view` 偏好记住；
- 移动端它该和聊天/设置并列，而不是埋在一个按钮后面。

Panel 侧为此加了通用扩展点 `PanelMainView`（见 `Extensions/registry` 的
`PanelMainView` 文档）：任何扩展都能注册主区视图，宿主负责顶栏按钮、移动端菜单、
持久化与门控。扩展这边只有一处：`mainViews` 里声明 `id / label / icon / component / when`。

视图组件拿到三个 prop：`teamId` / `sessionId` / `core`。**布局责任在视图自己**——
宿主只给一个零 padding 的 flex 容器，滚动与间距都归视图。

## 目录选择

两段分开摆（`已配置的文档根` / `浏览目录`）：进一个已知位置、与找一个新位置是
两个不同的问题，混在一屏里会让人不知道哪些按钮是干什么的。

根按钮是**两行**——主行是模块算出的不重名短名（`DocRenderer/document`），副行是
`~` 缩写的完整路径。两个根都叫 `document` 时只显示末段就分不清了。

上次打开的根记在 `localStorage`（按 `core|team` 分键，只记路径不记内容）。恢复时
会先核对它是否仍在 `roots` 里——配置删掉那个目录后不会再往一个非法根上冲。

## 检查

工具栏的「检查」按钮发 `ProDocCheck`，报告走模块事件 `prodoc.report`，按错误 /
警告 / 提示分组展示。有**错误级**问题才自动展开——纯警告是草稿的常态，不该挡
住画布。

同一份检查也暴露成宿主工具 `prodoc_check`，模型可以直接调用。

## 已知缺口：冲突面板的磁盘对照

`DocGraphViewer` 触发冲突面板时会调它源码里硬编码的
`fetch('/__prodoc_api/raw')` 读磁盘原文——面板没有这个端点，所以并排对照的
"磁盘一侧"是空的。

不影响保存与合并（那两条走我们自己的 handler），"采用我的版本"仍按
`disk = undefined` 强制覆盖。补齐要给组件加 `readHandler` prop：那是上游改动，
不在这里偷偷改 vendored 产物（下次同步 dist 就丢）。
