---
prodoc: true
id: config
title: 配置与沙箱
group: 接口契约
---

# 配置与沙箱

配置来自 `modules.toml` 的 `[module.config]`，反序列化到 `lib::Config`
（`deny_unknown_fields`：字段名写错会报错，不静默忽略——否则让人以为 `max_file` 生效了）。

## 配置项

| 键 | 缺省 | 作用 |
|---|---|---|
| `roots` | 空 | **发现范围**，同时是沙箱边界 |
| `require_marker` | `true` | 是否要求目录自己声明 `prodoc: true` |
| `discover_depth` | `4` | 发现时向下递归的最大层数（根自身是第 0 层） |
| `max_file_bytes` | 1 MB | 单文件字节上限 |
| `max_files` | 2000 | 篇数上限 |
| `max_total_bytes` | 32 MB | 全量总字节上限 |

```toml
[[module]]
id = "echo-agent.prodoc"
kind = "builtin"
name = "ProDoc 文档群"
enabled = true

[module.config]
roots = ["/srv/docs", "/root/work/agent-module-prodoc"]
# require_marker = true
# discover_depth = 4
```

配置解析失败**不挡启动**：warn 一条、按缺省上限继续（与宿主「配置缺失不阻断开机」同款
取舍）。所以「我改了上限但没生效」要先看启动日志里那条 warn。

## 发现范围即沙箱

`roots` 同时被两件事使用，必须一致：

- 比沙箱宽 → 面板会列出点不开的项
- 比沙箱宽的反面（沙箱更宽）→ 用户能用别的路径绕开识别

沙箱校验（`check_allowed`）对**两边都 canonicalize** 再比较前缀：软链成环或两个根互相包含时
不会误判，`/docs` 也不会匹配到 `/docs-old`。校验覆盖「根本身及其子目录」，空配置回退
`HOME`——容器里就是 `/root`，等于把整棵家目录当候选，慢且噪音大，所以实例配置里建议显式写
`roots`。

## 标记校验

`require_marker` 的默认值来自一次真实的误判：任何仓库根都有 `README.md`，「目录里有 `.md`」
不能作为「这是文档群」的判据。判据是**这个目录自己的**直接子文件里，参数区有没有
`prodoc: true`：

- 必须写在参数区里，正文里出现同样字样不算（走参数区解析器，与 `id` / `link` 同一份规则）
- 只看**这一层**，不是「整棵树里有没有」——后者会让 `<repo>/README.md` 这种没有标记、但
  子孙是文档群的仓库根通过校验，等于把禁令变成空话
- 值只认 `true`（也接受 `"true"`，大小写不敏感）
- 参数区未闭合时整篇按正文处理，声明不生效

拒绝时的文案会直接写出字段名（`至少一篇文档的参数区要写 prodoc: true`），作者当场就能改。
把 `require_marker` 关掉等于回到旧行为，只应在迁移期临时使用。

## 发现规则

`discover` 在 `roots` 下递归找文档群，每条规则都挡掉一类误判：

- **命中即止**：目录自己带标记就不再往下走。文档群是嵌套的（`<repo>/document/{core,modules}`），
  不剪枝会把同一个群报成十几个根
- **realpath 去重**：符号链接成环或两个根互相包含时不重复报
- **深度上限** `discover_depth`：文档群实际都在 1–2 层内
- **跳过隐藏目录与产物目录**：与扫描器同一份名单（`node_modules` / `.git` / `.cache` /
  `dist` / `build` / `target`）
- **子目录排序后遍历**：`read_dir` 顺序随文件系统变，不排会让面板列表每次不同

标记探测只读每个文件**前 8 KB**、最多看**前 50 篇**（排序后取）：参数区总在最前面，没必要
为一个布尔判断读完整篇。

## 根列表的显示名

两个根可能都叫 `document`——本机就真实存在（DocRenderer 的文档群与 Core 仓自己的
`document/`）。只显示末段的话面板里是两个一模一样的按钮。

所以 `RootEntry.label` 取**能区分的最短后缀**：从末段开始，只要有重名就把所有候选一起往上加
一层，直到全不相同。`hint` 是 `~` 缩写的完整路径，用于第二行确认。UI 侧是两行按钮 + 顶栏
label 主、hint 次。
