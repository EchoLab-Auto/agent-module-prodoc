//! 扩展自己的测试配置（2026-11）。
//!
//! 此前测试借 Panel 的 `web/` 目录跑，但 Panel 的 `vitest.config.ts` 已经改成
//! **只跑面板自己的测试**（扩展改成运行期加载后，面板不再编译扩展源码）。
//! 所以这里自带一份最小配置：只跑 `tests/`，纯函数层不需要 DOM。
//!
//! `@echo-agent/panel` 由测试自己 `vi.mock`，不需要 alias 到 Panel 源码——
//! 真实模块一加载就会牵出 socket 与全局 state。

import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['tests/**/*.test.ts'],
    environment: 'node',
  },
});
