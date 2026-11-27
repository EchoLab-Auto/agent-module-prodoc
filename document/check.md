---
prodoc: true
id: check
title: 文档图检查
group: 实现
---

# 文档图检查

`ProDocCheck` 在 Rust 里重新实现了一遍 ProDoc CLI 的**图结构**检查。动机很具体：agent 需要
在 Core 进程内检查文档图，它不能起一个 Node 进程去跑 CLI。

## 检查项

| 规则 | 级别 | 判据 |
|---|---|---|
| `no-documents` | error | 目录下一篇 `.md` 都没有 |
| `duplicate-id` | error | 两篇解析出同一 id（后者覆盖前者，按路径字典序） |
| `broken-link` | error | `link` 引用了不存在的文档 |
| `self-link` | error | `link` 指向自身 |
| `cycle` | warning | 强连通分量成员 > 1（Tarjan SCC） |
| `empty-document` | warning | 只有参数区、正文为空 |
| `long-document` | warning | 超过 800 行（与 CLI 的 `maxLines` 缺省一致） |
| `orphan` | warning | 既非起点也非终点，在大图上孤立漂浮 |
| `multi-group` | warning | 声明了多个 `group`，仅取第一个 |
| `multi-parent` | warning | 同一文档被 ≥2 篇 `link` 指向（**本模块独有**） |
| `group-scattered` | warning | 同组成员深度跨度 ≥2（**本模块独有**） |
| `no-frame` | info | 没有参数区，id / 标题 / 坐标靠推断 |
| `partial-position` | info | 只声明了 `x` 或 `y` 之一 |

排序固定为 error → warning → info，同级按文件路径，便于稳定对比。

## 两条独有规则的由来

CLI 没有这两条，而它们对应的几何后果是**必然**的，不是「画得好看不好看」：

- **`multi-parent`**：每个框只占一个位置，被多个父指向时它只能折中，于是每个父都会
  偏离子群中部。CLI 的 SKILL 当时只写了「用 `grep` 数入度」——这条不该让作者自己数。
- **`group-scattered`**：组区域 = 成员包围盒，而它是在布局**之后**才算的
  （`computeGroupRegion`）；布局求解器的输入只有框与连线，**不知道有组**。层带是整条
  水平带，所以跨 `d..d+k` 层的组，其 y 区间必然覆盖中间每一层——中间层的非成员框被一起
  框进去。跨度 1（一个父 + 它的直接子）通常是干净的 T 形，不报。

深度用的是与布局同一套规则（SCC 归约后的最长路径 + 1，`layer_depths`）：这个数字
会出现在给作者看的 warning 里，按别的算法算他会去数画布上的层，然后怀疑工具坏了。

实测（Core 仓 `document/`，68 篇）：`Core` 组 30 员跨第 0–4 层、`前端` 29 员跨第 1–6
层、`工程` 6 员跨第 0–2 层；`multi-parent` 命中 `协议与数据流`。两个大组覆盖 87% 的
文档——那时「这堆是一组」的信息量归零，只剩交错。

## 与 CLI 的关系

| 检查项 | 本模块 | CLI |
|---|---|---|
| 断链 / 自连 / 重复 id | 有 | 有 |
| 环（SCC） | 有 | 有 |
| 孤立 / 空正文 / 超长 / 缺参数区 / 单轴坐标 / 多 group 声明 | 有 | 有 |
| group 的显式几何冲突 | **无** | 有 |
| `multi-parent` / `group-scattered` | **有** | 无 |

group 的几何冲突是**纯布局外观**问题（框画在哪、围成什么形状），不影响图的结构与连通性；
复刻要连 `parseGroupEntry` 的几何语法一起搬，收益不抵风险。需要那一项请跑 CLI。

反过来，上面两条是本模块加的：跑 `cli_parity` 对照时**预期本模块多报**，不要当成
不一致（`tests/cli_parity.rs` 的文件头写着这一点）。
已知有意的差异：`broken-link` 的 `file` 字段填**声明方**（CLI 留空，它拿目标名反查不到）——
排查时更想知道「谁写错了」。

## 解析对齐点

复刻就有分叉风险，所以每条都与 `frame.ts` / `graph.ts` 对齐：

- 参数区必须从**第 0 字节**开始，两个 `---` 行夹住；找不到闭合就整篇按正文
- 值按「整数 → 浮点 → 布尔 → 内联列表 → 引号串 → 裸串」判定
- 引用归一化：剥 `<>`、去 `#锚点` / `?查询`、URL 解码、反斜杠转斜杠、折叠 `.` / `..`、外链跳过
- 先按 id 全局查表，失败再按声明方目录做相对解析

少一条就会把合法文档报成断链。

## 报告与两条出口

`Report` 有 `issues` 与 `stats`：`documents` 是**唯一 id 数**（与 CLI 的 `graph.boxes.length`
同义，不是文件数）、`relations`、`groups`、`errors`、`warnings`。

- 面板：`prodoc.report` 事件（`ok = !has_errors()`）
- 工具代理：命令回执里的 `CheckReceipt`（同一份数据的扁平形态，字段名与宿主侧共用类型，
  改名会让两侧一起编译失败，而不是让工具静默报告「0 个错误」）

逐**框**检查而非逐文件：重复 id 时被覆盖的那份不再报一遍——否则同一个问题报两遍，且报在
一个图上不存在的框上。

`tests/cli_parity.rs` 是手动跑的对照入口：`#[ignore]` + `PRODOC_FIXTURE` 指向一个真实文档群，
把 `check` 的结果与 CLI 输出逐条比对，文件头记录实测结论。CI 不跑它（需要 Node 与 CLI）。

```bash
PRODOC_FIXTURE=/tmp/fixture cargo test --test cli_parity -- --ignored --nocapture
```

注意它会因为 `multi-parent` / `group-scattered` 而**多报**若干条——那两条 CLI 没有，
不是分叉。
