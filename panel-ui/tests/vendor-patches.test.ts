//! vendored 产物的补丁守卫。
//!
//! `vendor/prodoc-renderer/` 与 `vendor/prodoc-core/` 是上游 `DocRenderer` 仓的
//! **构建产物**，我们直接改了其中的 dist（理由与每一处改动见
//! `vendor/patches/README.md`）。这类改动的失效方式是静默的：升级 vendor 时把
//! `dist` 整个覆盖，画布照旧能打开、行为却退回上游默认——只有用户体感不对时才
//! 发现，而那时已经不知道是哪一次升级弄丢的。
//!
//! 所以每条补丁在这里留一条文本断言：覆盖 dist 后 `npm test` 直接红。

import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const renderer = readFileSync(
  fileURLToPath(new URL('../vendor/prodoc-renderer/index.js', import.meta.url)),
  'utf8',
);
const css = readFileSync(
  fileURLToPath(new URL('../vendor/prodoc-renderer/index.css', import.meta.url)),
  'utf8',
);

describe('补丁 2026-11-relayout-save（分层重排落盘）', () => {
  it('「💾 保存布局」的落盘出口还在', () => {
    // `saveRelayout` 是补丁新增的函数名，上游没有。
    expect(renderer).toContain('saveRelayout');
  });
});

describe('补丁 2026-11-pan-feel（画布平移手感）', () => {
  it('画布用受控平移（infinite），不是原生滚动', () => {
    // 关键：`infinite: ""` 在 `resolveProp = e ?? n ?? u` 里**不触发回落**，
    // 空串是 falsy → 画布退回原生 `scrollLeft` 流。原生产品的滚动带系统级
    // 阻尼/加速曲线，触屏拖动还会被 `pointerType === "touch" && !infinite`
    // 直接挡掉，体感就是「滑动太慢 / 拖不动」。
    expect(renderer).toContain('infinite: !0');
    expect(renderer).not.toContain('infinite: "",');
  });

  it('滚轮平移由画布自己派发 panBy（可调倍率），并拦下冒泡', () => {
    expect(renderer).toContain('onWheel: u[27] ||=');
    expect(renderer).toContain('c.panBy(-e.deltaX * n * 1.5, -e.deltaY * n * 1.5)');
    expect(renderer).toContain('e.preventDefault(), e.stopPropagation();');
  });

  it('Ctrl / Meta + 滚轮仍然交回 canvas 做缩放', () => {
    // 抢事件时最容易踩的一脚：把缩放也拦掉，用户就再也缩放不了。
    expect(renderer).toContain('if (e.ctrlKey || e.metaKey) return;');
  });
});
describe('补丁 2026-11-group-loose（组框误伤时降级）', () => {
  it('算出了「组区域里有非成员框」的集合，且折叠态不启用', () => {
    expect(renderer).toContain('looseGroups = t(() =>');
    // 判据是中心点，不是矩形相交：内边距只有 24/34px，相交会误伤一圈邻居。
    expect(renderer).toContain('let cx = box.x + box.w / 2, cy = box.y + box.h / 2;');
    // 折叠态：组成员不渲染，画布上不存在交错，不该降级（否则超节点也没了）。
    expect(renderer).toContain('if (V.value.size > 0) return out;');
  });

  it('模板按组名打 loose class', () => {
    expect(renderer).toContain('"pd-doc-group--loose": looseGroups.value.has(e.name)');
  });

  it('loose 时去掉虚线矩形与尺寸手柄，但保留组名 chip 的点击', () => {
    expect(css).toContain('.pd-doc-group--loose{border-color:transparent;background:none;pointer-events:none}');
    expect(css).toContain('.pd-doc-group--loose .pd-doc-group__label{pointer-events:auto');
    expect(css).toContain('.pd-doc-group--loose .pd-doc-group__resize{display:none}');
  });
});
describe('补丁 2026-11-depth-window（层带 + 层级窗口）', () => {
  it('组件接受 maxDepth，且缺省（不传）时不过滤', () => {
    expect(renderer).toContain('maxDepth: {}');
    // 判定必须是「假值就放行」：面板默认不传 → 完整图，与旧行为一致。
    expect(renderer).toContain('if (!limit || !Number.isFinite(limit) || limit <= 0) return e;');
  });

  it('过滤发生在折叠（ct）之前：布局真的少算节点，不是藏起来留空洞', () => {
    const depth = renderer.indexOf('hiddenTail');
    const collapse = renderer.indexOf('Ke = t(() => ct({');
    expect(depth).toBeGreaterThan(0);
    expect(collapse).toBeGreaterThan(depth);
  });

  it('可见框上标出被藏起来的子节点数（+N）', () => {
    expect(renderer).toContain('hiddenTails(t.id) ?');
    expect(css).toContain('.pd-doc-box__tail{');
  });

  it('层带由布局后的 K 反推，并排除折叠出来的组超节点', () => {
    expect(renderer).toContain('layerBands = t(() =>');
    expect(renderer).toContain('if (box.attrs && box.attrs.type === "group") continue;');
    expect(css).toContain('.pd-layer-band{');
  });

  it('层带标签可点击：把窗口收到该层（depth+1）', () => {
    expect(renderer).toContain('emits: ["navigate", "save", "depth-pick"],');
    expect(renderer).toContain('se("depth-pick", b.depth + 1)');
  });
});
