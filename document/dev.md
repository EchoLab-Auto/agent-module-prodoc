---
prodoc: true
id: dev
title: 开发与测试
group: 交付
---

# 开发与测试

## 本地命令

```bash
cd /path/to/agent-module-prodoc
cargo test                              # 单元 + 集成（CLI 对照默认 ignore）
cargo clippy --all-targets -- -D warnings
```

Clippy 到 `-D warnings` 是硬线：检查器与扫描器都有大量边界判断，「先放过一条警告」在这类
代码里会迅速变成分叉源。

## CLI 对照测试

`tests/cli_parity.rs` 手动跑，需要 Node 与 `echo-prodoc` CLI：

```bash
PRODOC_FIXTURE=/tmp/fixture cargo test --test cli_parity -- --ignored --nocapture
```

CLI 的检查逻辑在 `prodoc-core/graph.ts` / `prodoc-cli/check.ts`，本仓在
[`check.rs`](./check.md) 重实现了一份**图结构子集**。复刻必然有分叉风险，所以：

1. 边界刻意划死（group 几何冲突不做）
2. 每条解析规则都对着源码逐条抄（参数区起点、值类型判定、引用归一化）
3. 有对照入口，且文件头记录实测结论

改检查逻辑时**先跑一次对照**再改，比改完再猜哪条不一致便宜得多。

## 框架类型对齐

`echo-module` / `echo-defs` 按 tag 取自 `echo-fabric-arch`。本地联调框架改动时，把依赖临时
换成 `path`（`Cargo.toml` 里留了注释掉的写法），联调完必须换回 tag——否则 Core 里会出现
两份框架类型，报错信息通常指向一个看起来完全无关的地方。

## 面板 UI

在 `panel-ui/` 目录里构建与测试（详见 [面板 UI](./panel-ui.md)）：

```bash
cd panel-ui
npx vite build
npm test
```

## 文档群自检

`document/` 自身就是一个文档群，改完文档可以直接验：

```bash
# 面板里打开本仓的 document/，或用 CLI
echo-prodoc view ./document
```

模块侧的检查（与本仓文档群一致）：

```bash
# 需要 Core 已挂载 echo-agent.prodoc，且本仓在 roots 内
prodoc_check /root/work/agent-module-prodoc/document
```

检查项的完整列表见 [文档图检查](./check.md)。写文档时最容易踩的两条：

- **多父**：一篇文档只应被一个 `link` 指向（`check` 不查多父，只能 `grep` 数入度）
- **分组必须连通**：组框几何 = 成员外接矩形，成员被排到不同层会把别的文档框进来

## 提交前清单

- [ ] `cargo test` 全绿；`cargo clippy --all-targets -- -D warnings` 干净
- [ ] 动了检查 / 扫描：跑过 `cli_parity`
- [ ] 动了 `SKILL.md`：缓存区与 `check.rs` 的解析规则一致
- [ ] 动了 `document/`：`prodoc_check` 无 error
- [ ] `module.toml` 的 id / tag 没动（运行期标识与构建归属）
