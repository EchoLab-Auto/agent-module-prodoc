# vendored 产物的本地补丁

`vendor/` 下两个目录是上游 `DocRenderer` 仓的**构建产物**（不是源码，也不是 npm
依赖），所以给它们打补丁时改的是压缩后的 dist。这里登记每一处补丁，避免升级
vendor 时静默丢失。

升级流程（见 `../../README.md`）覆盖 `index.js` / `index.css` 之前，先确认下面
每一条在新产物里仍然需要；需要就把对应改动重放到新产物上。

三份补丁的失效都有测试兜底：`tests/vendor-patches.test.ts` 给每处留一条文本
断言，覆盖 dist 后 `npm test` 直接红。

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
**为什么不直接换上游新构建**：同上一节结论（vendored 的是更晚的版本，缺功能）。

## `2026-11-group-loose.patch` — 组框误伤时降级

**症状**：同一个组框里塞着别的组的文档，两个组的虚线矩形互相重叠；跨三层的组
干脆把中间整条层带都框进来，画布上分不清"这堆是一组"还是"只是被框住了"。

**原因**：不是画错，是**布局不知道有组**。分层布局的输入只有框与连线两样
（`computeLayeredLayout(boxes, relations)`），组区域是布局跑完之后按
`computeGroupRegion(members)` = 成员包围盒 + 内边距（`x-24, y-34, w+48, h+58`）
补算的。层带是整条水平带，所以一个跨 `d..d+k` 层的组，其 y 区间覆盖中间每一层。

实测（Core 仓 `document/`，68 篇）：4 个组全部跨层，`Core`（30 员跨第 0–4 层）与
`前端`（29 员跨第 1–6 层）两个框覆盖了 87% 的文档。

**补丁内容**（`vendor/prodoc-renderer/index.js` + `index.css`）：

1. 新增 `looseGroups` computed：对每个组，检查是否存在**非成员框的中心点**落在组
   区域内，命中则把组名记入集合。
   - 判据取中心点而不是矩形相交：内边距只有 24/34px，相交会把紧贴的一圈邻居全算
     成侵入，几乎每组都要降级。
   - 折叠态（`V.value.size > 0`）直接返回空集：组成员不渲染，画布上不存在交错，
     而且那时渲染的是超节点，不该被降级掉。
2. 模板按组名打 `pd-doc-group--loose`。
3. CSS：loose 时去掉虚线边框与底色、关掉整块的 `pointer-events`，保留组名 chip
   的点击（折叠入口）并隐藏尺寸手柄（区域已经不是作者的意图，调尺寸没意义）。

**设计取舍**：降级是"不说假话"，而不是"修正几何"。一个矩形画不出非凸形状——
把 `Core` 组摊平到 5 层里还是个矩形，怎么调都会盖住别人。所以要么让组别跨层
（`check` 的 `group-scattered` 负责提醒作者），要么就别画框（本条负责不让它撒谎）。

**为什么不改成"组参与布局"**：那要把 group 塞进 `computeLayeredLayout` 的层内
排序（同组成员连续成块），是上游的算法改动，动的是求解器而不是渲染。本仓
vendored 的是构建产物，改算法等于自己维护一个 fork。

**已知残留**：折叠态不降级——若一组跨层且被折叠，展开后才会显示为 chip。
这与"折叠才是组的主场"一致，不修。

## `2026-11-pan-feel.patch` — 画布平移的手感

**症状**：文档图画布「滑动太慢 / 拖不动」——滚轮推一下走得很少，触摸屏与
触控板用手指拖根本不动。

**原因**：渲染器给 `NeumorphismCanvas` 传的是 `infinite: ""`。ui-frame 的
`resolveProp` 语义是 `props ?? 主题配置 ?? 缺省`，空字符串**不是** `null` /
`undefined`，所以不会回落到缺省值——`Boolean("") === false`，画布实际处于
**非** 无限画布模式：

- 平移走 viewport 的原生 `scrollLeft/scrollTop`，带操作系统级的滚动阻尼/加速
  曲线（慢速移动时位移很小），而不是逐事件 1:1 的受控平移；
- ui-frame 的拖拽启动条件里有一条 `pointerType === "touch" && !infinite` 直接
  return——触屏拖动完全无效，只剩原生滚动；
- 渲染器自己的 `Ze` 视口状态从 `getView().panX/panY` 取值，非无限模式下这两个
  恒为 0，于是「视口矩形」永远算在左上角。文档群超过 40 个框时（`ut` 开启虚拟
  化）滚到别处，框会被当作视口外而不渲染——大图上表现为「框莫名消失」。

**补丁内容**（`vendor/prodoc-renderer/index.js`，两处）：

1. `infinite: ""` → `infinite: !0`。画布进入受控平移：`content` 用
   `translate(panX, panY) scale(zoom)`，拖动与滚轮都是逐事件 1:1；触屏拖动同时
   恢复正常（`--infinite` 类还带来 `touch-action: none`）。fit 按钮走
   `contentBounds` 分支，渲染器本来就传了 `content-bounds`，无需额外改动。
2. stage 上新增 `onWheel`（`u[27]` 缓存）：在事件冒泡到 viewport 之前
   `preventDefault + stopPropagation`，自己按 **1.5 倍**调
   `canvasRef.panBy(-deltaX*k, -deltaY*k)`。为什么不用 canvas 内建的滚轮平移：
   它固定在 1:1，用户实测偏慢，而倍率只能在我们自己这层定。

   三个必须保留的细节：
   - `Ctrl` / `Meta` 按下时**直接 return**（那是缩放，拦掉就再也缩放不了）；
   - `textarea` / `input` / `.pd-doc-view` / `.pd-search-body` 内部的滚轮放行
     （那些区域要自己的原生滚动）；
   - `deltaMode === 1 ? 16 : 1`：行模式滚轮（Firefox）要换算成像素。

**已知残留**：`panBy` 只在 `infinite` 生效。这两处改动是配套的——只改倍率不改
`infinite` 会变成「滚轮被拦下来但画布不动」，比原来更糟。`tests/vendor-patches.test.ts`
把这条关系钉住了。

**调手感**：慢就改 `c.panBy(... * 1.5)` 里的 `1.5`，快就往下调。改完必须跑
`npm test`（断言里粘了这个字面量，忘了同步会红）。

**为什么不直接换上游新构建**：同上一节结论（vendored 的是更晚的版本，缺功能）。
