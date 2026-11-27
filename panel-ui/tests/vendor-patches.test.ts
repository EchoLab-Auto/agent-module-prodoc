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
