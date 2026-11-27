---
prodoc: true
id: scan
title: 扫描与上限
group: 实现
---

# 扫描与上限

`bundle::scan` 把一棵目录读成 `Bundle`：`files` 是 `相对路径 → 完整原文`（**含**参数区），
外带 `skipped` 与 `stats`。

## 与 CLI 同一套语义

与 ProDoc CLI 的 `loadMarkdownFiles`（`prodoc-cli/src/server.ts`）逐条对齐：

- 跳过 `node_modules` / `.git` / `.cache` / `dist` / `build`，外加 `target` 与隐藏目录。
  漏掉 `target` 与 `node_modules` 会让扫描在大型仓库里跑上千个文件、直接撞上限
- `.md` 后缀**大小写不敏感**
- `realpath` 去重防符号链接环
- 相对路径统一用 `/`（Windows 上反斜杠也只当分隔符）

扫描规则改动必须与 [文档图检查](./check.md) 同步：检查器的入参就是扫描产物，两套规则一旦
分裂，表现是「面板里看得到、检查说没有」。

## 上限

| 项 | CLI | 本模块 |
|---|---|---|
| 单文件 | 无 | 1 MB |
| 篇数 | 无 | 2000 |
| 总量 | 无 | 32 MB |

三项都可以在 `[module.config]` 里放宽（见 [配置与沙箱](./config.md)）。

超限**明确报出被跳过的文件**（`skipped` 带原因，`stats.truncated = true`），绝不静默截断：
截断的文档图会缺节点，而作者看不出是「被截了」还是「本来就少写了一篇」。

## 快照结构

| 字段 | 说明 |
|---|---|
| `root` | 文档根（绝对路径，已规范化） |
| `files` | `BTreeMap`，键为相对路径 → 排序稳定，两次扫描结果可比 |
| `skipped` | `{path, reason}` 列表 |
| `stats` | `documents` / `bytes` / `skipped` / `truncated` |
| `prodoc` | 本快照是否声明为文档群（扫描器填，打开时以探头结果为准） |
| `team_id` | 命令层补上，面板按它过滤 |

原文**逐字保留**：换行符、BOM、末尾换行一个字节都不规范。写盘的冲突判定是整篇全等比对，
读时做过规范化会让 `base` 永远不等于磁盘内容——见 [写盘与冲突](./write.md)。

## 标记探测

`dir_has_marker` 判断「这个目录自己」是不是文档群：只看直接子 `.md`，只读每个文件前 8 KB，
最多看排序后的前 50 篇。判据走 `check::parse_frame`，与检查器的 `no-frame` 判定共用一份
规则——不会出现「检查器认为有参数区、探测器认为没有」。

探测与扫描一起进 `spawn_blocking`：两者都是文件 IO，分两次调度只会多一次开销。

## 扫描失败

`ScanError` 只有两种，都转成 `prodoc.error` 并让命令 `Err`：

- `RootUnavailable`：根不存在 / 不是目录 / 无权限
- `Empty`：一篇 `.md` 都没找到——**这是错误，不是空文档群**。空画布会让人以为渲染坏了
