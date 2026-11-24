//! echo-agent.prodoc 的 Panel UI 扩展构建。
//!
//! 与工作区模块的配置同源（那份写着完整的"为什么"）：
//!
//! - **产物固定 `dist/index.js`**：Core 侧登记的 bundle 路径不带 hash，
//!   面板靠 import map 加载——名字一变就失联。
//! - **共享模块 externalize**：`vue` / `@echo-agent/panel*` /
//!   `@echolab-auto/ui-frame*` 由面板的 import map 提供（单一实例：两份 vue
//!   就是两套响应式，两份 store 就是改自己的影子状态）。
//! - **CSS 内联进 JS**：浏览器原生 ES 模块不支持 `import "x.css"`，而动态
//!   `import()` 的产物不走 vite 的 preload helper。

import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

/** ui-frame 的全局样式：面板已全局引入，扩展再引一次就是重复。 */
const UI_FRAME_CSS = /^@echolab-auto\/ui-frame\/(dist\/)?(style|tokens|base)\.css$/;

/**
 * 把抽取出来的 CSS 注入到 JS 里（运行期追加 `<style>`）。
 *
 * 同时**丢掉 ui-frame 的全局样式导入**：面板 `main.ts` 已经全局引过
 * `ui-frame.css`，每个扩展再内联一份 312 KB 是纯浪费，而且会让"谁覆盖谁"
 * 取决于扩展加载顺序。
 */
function inlineCss(): import('vite').Plugin {
  return {
    name: 'echo-panel-ui-inline-css',
    enforce: 'post',
    generateBundle(_options, bundle) {
      const styles: string[] = [];
      for (const [file, asset] of Object.entries(bundle)) {
        if (asset.type === 'asset' && file.endsWith('.css')) {
          styles.push(String(asset.source));
          delete bundle[file];
        }
      }
      const payload = JSON.stringify(styles.join('\n'));
      const injection =
        styles.length === 0
          ? ''
          : `\n// 扩展的样式（构建期从 SFC 抽出，运行期插进 <head>）\n` +
            `const __css = ${payload};\n` +
            `if (typeof document !== 'undefined') {\n` +
            `  const el = document.createElement('style');\n` +
            `  el.dataset.panelExtension = ${JSON.stringify('echo-agent.prodoc')};\n` +
            `  el.textContent = __css;\n` +
            `  document.head.appendChild(el);\n` +
            `}\n`;
      for (const chunk of Object.values(bundle)) {
        if (chunk.type === 'chunk' && chunk.isEntry) chunk.code += injection;
      }
    },
  };
}

export default defineConfig({
  plugins: [
    // 丢掉 ui-frame 全局样式导入（见 `UI_FRAME_CSS`）。必须在 vue 插件之前
    // 生效——它在解析阶段就要把这些 import 抹掉，而不是等到打包。
    {
      name: 'echo-panel-ui-drop-ui-frame-css',
      enforce: 'pre',
      resolveId(source) {
        if (UI_FRAME_CSS.test(source)) return '\0ui-frame-css-dropped';
        return null;
      },
      load(id) {
        return id === '\0ui-frame-css-dropped' ? 'export {}' : null;
      },
    },
    vue(),
    inlineCss(),
  ],
  build: {
    lib: { entry: 'index.ts', formats: ['es'], fileName: () => 'index.js' },
    outDir: 'dist',
    emptyOutDir: true,
    cssCodeSplit: false,
    rollupOptions: {
      external: (id) =>
        id === 'vue' ||
        id.startsWith('@echo-agent/panel') ||
        (id.startsWith('@echolab-auto/ui-frame') && !id.includes('.css')),
    },
    sourcemap: true,
    target: 'es2022',
  },
});
