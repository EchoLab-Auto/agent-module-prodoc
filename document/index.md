---
prodoc: true
id: overview
title: ProDoc 文档群模块
link: ["commands | 接口契约", "impl | 实现拆解", "deliver | 交付与集成"]
---

# ProDoc 文档群模块

把一个 Markdown 目录读成 **ProDoc 文档图**，交给 Panel 的文档画布渲染。模块 id 为
`echo-agent.prodoc`，crate 名 `agent-module-prodoc`——**运行期标识不随 crate 名变**，它出现在
配置、面板门控与持久化状态里。

## 它解决什么问题

ProDoc 原本是独立 CLI（`echo-prodoc view`），自带一个 dev server 负责读盘、写盘、热更新。
嵌进 Panel 后这三件事必须换成宿主提供的通道：

| 能力 | CLI | 本模块 |
|---|---|---|
| 读盘 | dev server 直接读 | Core 扫目录，模块事件推快照 |
| 渲染 | 浏览器 | 浏览器（不变，`DocGraphViewer`） |
| 写盘 | dev server 直接写 | Core 原子写 + 冲突回执 |
| 合并 | 服务端跑 diff3 | **前端**跑 diff3，后端只做逐字比对 |

读盘必须在 Core 的原因：Panel 是独立进程，没有文件系统权限模型，也不该为了看个文档去开一个
文件接口。同理写盘也只接受「根 + 相对路径 + 新内容 + base」，不暴露任意路径写。

## 数据流

```prodoc-flow
graph LR
  UI[文档面板|/panel-ui.md] --> Cmd[命令通道|/commands.md]
  Cmd --> Scan[目录扫描|/scan.md]
  Scan --> Evt[模块事件|/events.md]
  Evt --> UI
  UI --> W[写盘与冲突|/write.md]
```

读路径：扩展 `sendCommand(RequestProDocBundle)` → Core 扫目录 → 事件 `prodoc.bundle` →
画布。写路径：画布 → `SaveProDocFile`（带 `base`）→ 冲突时回执附磁盘内容 → 前端合并后重发。

**内容不走命令回执**：宿主命令分发只认 `Err`（转错误事件），`Ok(value)` 的返回值被丢弃。
这条如果搞错，表现是「命令发了、界面永远转圈」，日志一片安静。详见 [事件协议](./events.md)。

## 图模型一句话

目录即图、文件即框、`link` 即连线。框的属性（id / 标题 / 坐标 / 分组）写在文件最前方的
框架参数区；文档群靠参数区里的 `prodoc: true` 自动识别。完整格式规范见本 crate 贡献的技能
`prodoc-format`（`crates/agent-module-prodoc/skills/prodoc-format/SKILL.md`）。

## 文档地图

| 分支 | 入口 | 覆盖内容 |
|---|---|---|
| 接口契约 | [命令契约](./commands.md) | 七个命令、八条事件、配置与沙箱 |
| 实现拆解 | [代码结构](./impl.md) | 扫描、写盘、检查三个纯逻辑单元 |
| 交付与集成 | [交付形态](./deliver.md) | 面板 bundle、技能、宿主接线、开发测试 |

## 与 ProDoc CLI 的刻意差异

| 项 | CLI | 本模块 |
|---|---|---|
| 扫描上限 | 无 | 单文件 1 MB / 2000 篇 / 32 MB |
| 跳过目录 | 5 个固定名 | 同样 5 个 + `target` + 隐藏目录 |
| 交付 | 内联进虚拟模块源码 | 模块事件 |
| 检查 | 全部规则（TS） | 图结构子集（Rust） |

上限是必须的：CLI 是一次性页面加载，超量顶多慢；本模块走 WS 推送，一个大目录能瞬间生成
几十 MB 的事件消息，把面板的模块事件环形缓冲（200 条）撑爆。

## 这个目录自己就是一个文档群

`document/` 下的每篇参数区都写了 `prodoc: true`。落地方式有两种：

```bash
# 命令行（需安装 echo-prodoc CLI）
echo-prodoc view ./document

# 或在面板「文档」入口里选本仓：先由 echo-agent.prodoc 发现，再点开
```

建议整组都写标记：新加文档忘了声明时 id 会被推断、也能渲染，只是「为什么这篇不在图上」
会变得难查。
