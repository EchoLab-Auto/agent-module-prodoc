# agent-module-prodoc

ProDoc 文档群模块（EchoAgent 模块，**独立仓库交付**）。

从 `EchoAgentCore` 的内置模块 `source/modules/agent-module-prodoc` 迁出。文档扫描、
路径沙箱、单文件读写、文档图检查全部在本仓；Core 只保留通用接线（见「宿主侧接线」）。

## 它做什么

把一个 Markdown 目录读成 **ProDoc 文档图**所需的 `相对路径 → 全文` 映射，交给 Panel
的文档画布（`DocGraphViewer`）渲染。布局、边路由、搜索、编辑在浏览器里，**读盘在
Core**——Panel 是独立进程，没有文件系统权限模型，也不该为了看个文档去开文件接口。

## 能力

| 命令 | 语义 | 只读 |
|---|---|---|
| `RequestProDocBundle` | 扫描文档根，推回全量快照 | 是 |
| `BrowseProDocDirs` | 目录选择器逐层浏览（只列子目录） | 是 |
| `ProDocRoots` | 可打开的根列表 + 生效上限 | 是 |
| `ReadProDocFile` | 读单文件原文（冲突对照 / 合并基准） | 是 |
| `SaveProDocFile` | 保存单文件（带 `base` 冲突检测，原子写） | 否 |
| `DeleteProDocFile` | 删除单文件（`base` 不匹配则取消） | 否 |
| `ProDocCheck` | 文档图检查（断链 / 自连 / 重复 id / 环 / 孤立…） | 是 |

**内容不走命令回执**：宿主的命令分发只认 `Err`（转错误事件），`Ok(value)` 的返回值
被丢弃。所以快照、写回执、检查报告全部走模块事件：

| 事件 | 载荷 |
|---|---|
| `prodoc.bundle` | 文档群快照 |
| `prodoc.error` | 失败（面板显示为错误条） |
| `prodoc.roots` | 可打开的根 + 上限 |
| `prodoc.dirs` | 目录选择器的一层 |
| `prodoc.file` / `prodoc.saved` / `prodoc.deleted` | 写操作回执（带 `request_id`） |
| `prodoc.report` | 检查报告 |

## 扫描上限（与 ProDoc CLI 唯一的刻意差异）

| 项 | CLI | 本模块 |
|---|---|---|
| 单文件 | 无上限 | 1 MB |
| 篇数 | 无上限 | 2000 |
| 总量 | 无上限 | 32 MB |

CLI 是一次性页面加载，超量顶多慢；本模块走 WS 推送，一个几千文件的目录能瞬间生成
几十 MB 的事件消息，把面板的模块事件环形缓冲（200 条）撑爆。超限**明确报出被跳过
的文件**，绝不静默截断——截断的文档图会缺节点，而作者看不出是"被截了"还是"本来就
没写"。

## 写盘与冲突合并

与 ProDoc CLI 的 `write-api.ts` **逐条对齐**：

- `base` 缺省 = 无条件覆盖（新建文件走这条）；带 `base` 时整篇**内容全等**比对，
  不引入版本号 / mtime / hash；
- 提交内容与磁盘已一致 → 幂等成功（一次网络重试不该变成"冲突"）；
- 磁盘变了 → 冲突，**不写盘**，回执带 `disk`（磁盘当前内容）；
- 带 `base` 但文件已被删 → 冲突且**不重建**（重建等于撤销别人的删除）；
- 删除同理：`base` 不匹配即取消；删除**不做合并**。

写盘用同目录临时文件 + `rename`。直接 `write` 在中断时会把参数区截断，而下次扫描会
把它当成"没有参数区"的文件、节点 id 从文件名推断，图上的连线就全断了——这是 `.md`
特有的风险。

**三路合并不在本仓**：CLI 用的是 `prodoc-core/merge.ts` 的 265 行 hunk 级 diff3，
在 Rust 里复刻必然与它语义分叉（结果是"面板说合并成功、命令行说冲突"）。所以后端只
做"逐字比对 + 原子写"，冲突时把磁盘内容回给前端，由前端跑**同一份** `merge3` 后以
`base = disk` 重发。

## 文档图检查

在 Rust 里重实现了一遍 CLI 的**图结构**检查（agent 不能在 Core 进程内起 Node 进程
去跑 CLI），覆盖断链 / 自连 / 重复 id / 环（Tarjan SCC）/ 孤立 / 空正文 / 超长 /
缺参数区 / 单轴坐标。

复刻就有分叉风险，所以：

1. **边界刻意划死**：group 相关两项（多 group 声明 / 显式几何冲突）**不覆盖**——那是
   纯布局外观问题。需要那两项请跑 CLI。
2. **对照过真 CLI**：`tests/cli_parity.rs` 是手动跑的对照入口（`#[ignore]` +
   `PRODOC_FIXTURE`），文件头记录实测结果。

已知有意的差异：`broken-link` 的 `file` 字段填**声明方**（CLI 留空，它拿目标名反查
不到）——排查时更想知道"谁写错了"。

## Panel UI 扩展

`panel-ui/`：入口「文档」+ 一个 overlay。渲染器 vendored 在
`panel-ui/vendor/prodoc-renderer/`，diff3 在 `panel-ui/vendor/prodoc-core/`——原因与
升级步骤见 `panel-ui/README.md`。

安装（Panel 的安装器可重复给多个扩展）：

```bash
PANEL_UI_EXTENSIONS="/path/to/agent-module-workspace/panel-ui:/path/to/agent-module-prodoc/panel-ui" \
  bash ~/.local/libexec/echo-agent-panel/update.sh --source /path/to/EchoAgentPanel
```

## 宿主侧接线（EchoAgentCore）

1. Core 的 `Cargo.toml` 加依赖：

   ```toml
   agent-module-prodoc = { git = "https://github.com/EchoLab-Auto/agent-module-prodoc", branch = "main" }
   ```

2. 内置模块目录登记（`source/modules/core/src/builtin_modules.rs`）：

   ```rust
   push_module(&mut catalog, agent_module_prodoc::module(version))?;
   ```

3. `scripts/update.sh` 的 `expected_ids` 加 `echo-agent.prodoc`（与
   `BUILTIN_MODULE_IDS` 的一致性由 `instance_manifests` 测试守护）。

4. `modules.toml` 配置允许的文档根：

   ```toml
   [[module]]
   id = "echo-agent.prodoc"
   kind = "builtin"
   name = "ProDoc 文档群"
   enabled = true

   [module.config]
   # 空 = 只允许 HOME。不配这一项等于给面板开了一个"读全盘 .md"的口子。
   roots = ["/srv/docs"]
   ```

5. 宿主侧的 `prodoc_check` 工具是**转发壳**（`agent-module-loop` 的
   `agent/prodoc_tool.rs`），它按 `agent_module_prodoc::commands::CHECK_COMMAND`
   分发——工具名与命令名共用常量，避免两处字面量漂移。

模块 id 是 `echo-agent.prodoc`（运行期标识，出现在配置 / 面板门控 / 持久化状态里），
**不随 crate 名变**。

## 开发

```bash
cargo test                              # 单元 + 集成（含 CLI 对照，默认 ignore）
cargo clippy --all-targets -- -D warnings
PRODOC_FIXTURE=/tmp/fixture cargo test --test cli_parity -- --ignored --nocapture
```

框架类型（`echo-module`）按 tag 取自 `echo-fabric-arch`，与 Core 根 `Cargo.toml` 保持
同一 tag——同一进程里不能出现两份 `Box<dyn Module>`。本地联调框架改动时，把依赖临时
换成 `path` 即可（`Cargo.toml` 里留了注释掉的写法）。

Panel UI 扩展的构建与测试要从 Panel 的 `web/` 目录跑：

```bash
PANEL_UI_EXTENSIONS=/abs/path/to/agent-module-prodoc/panel-ui npm run build
PANEL_UI_EXTENSIONS=/abs/path/to/agent-module-prodoc/panel-ui npm test
```

## 许可

MIT
