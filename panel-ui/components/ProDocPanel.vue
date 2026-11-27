<script setup lang="ts">
//! ProDoc 文档群面板（**主区视图**，2026-11 起不再是浮层）。
//!
//! 两态：**选目录**（还没有文档根）与**看画布**（已打开）。
//!
//! ## 为什么是视图而不是浮层
//!
//! 浮层（`overlays`）适合"看一眼就关"的列表与表单。文档画布不是：它需要独占
//! 主区（可平移缩放的画布，边框和 padding 全是浪费）、"我在看哪个文档"该被
//! 顶栏选中态记住、移动端该和聊天/设置并列。这些是**视图**的特征。
//!
//! 布局责任也随之转移：浮层时代尺寸由 `SessionOverlay` 的 inset 算，现在宿主
//! 只给一个零 padding 的 flex 容器，滚动与间距全归本组件。
//!
//! ## 编辑：冲突合并交前端
//!
//! `saveHandler` 走 [`../write.ts`] 的 `saveFile`：带 `base` 提交，冲突时用
//! vendored 的 `merge3`（与 ProDoc CLI 同一份实现）自动合并后重发。
//!
//! 组件触发冲突面板（改动重叠、无法自动合并）时会调 `readDiskContent`——那是
//! 它在源码里硬编码的 `fetch('/__prodoc_api/raw')`，面板里没有这个端点，所以
//! 并排对照的"磁盘一侧"会显示为空。**这处缺口是有意留着的**：给组件加
//! `readHandler` prop 属于上游改动，不在这里悄悄改 vendored 产物（那样下次
//! 同步 dist 就丢）。缺了它不影响保存/合并，"采用我的版本"仍按 `disk=undefined`
//! 走强制覆盖。
//!
//! ## 目录选择：两种入口，**分开**摆
//!
//! 「已配置的根」与「浏览目录」是两个不同的问题（进已知的位置 / 找一个
//! 新位置），混在一屏里会让人不知道刚才那些按钮是干什么的。所以分成两段，
//! 各带一句说明。
//!
//! 根按钮是**两行**：`label` 是模块算出的不重名短名（`DocRenderer/document`），
//! `hint` 是 `~` 缩写的完整路径。本机就真实存在两个都叫 `document` 的根，
//! 只显示末段时它们一模一样。

import { computed, onMounted, ref, watch } from 'vue';
import NeumorphismButton from '@echolab-auto/ui-frame/components/NeumorphismButton';
import PanelIcon from '@echo-agent/panel/components/PanelIcon';
import { DocGraphViewer } from '../vendor/prodoc-renderer/index.js';
import '../vendor/prodoc-renderer/index.css';
import { activeEntry, activeReport, activeRoot, browse, check, closeRoot, openRoot, prodoc, refresh, requestRoots } from '../index';
import { breadcrumbs, describeBundle, parentPath } from '../prodoc';
import { deleteFile, saveFile } from '../write';

const props = defineProps<{
  teamId: string | null;
  sessionId?: string;
  /**
   * 当前 Core。由宿主传入而不是本组件读 `state.activeRegion`：
   * 视图在多上游聚合下要按"这一屏属于哪个 Core"取数据，宿主已经有一份
   * 确定的值，组件自己再读一次会多一个可能不同步的来源。
   */
  core?: string | null;
}>();

const core = computed(() => props.core ?? null);
const entry = computed(() => activeEntry(props.teamId, core.value));
const roots = computed(() => prodoc.roots);
const browseRoots = computed(() => (prodoc.browseRoots.length > 0 ? prodoc.browseRoots : prodoc.roots));
const crumbs = computed(() => breadcrumbs(prodoc.browsePath, browseRoots.value));
const summary = computed(() => (entry.value ? describeBundle(entry.value) : ''));
/** 当前根的显示信息（label 优先——绝对路径太长，扫读时认不出是哪个）。 */
const currentRoot = computed(() => activeRoot(core.value));
const report = computed(() => activeReport(props.teamId, core.value));
/** 检查面板是否展开（有错误时默认展开——那是用户最想看的）。 */
const reportOpen = ref(false);

/**
 * 层级窗口：只画到第 N 层（`all` = 全部）。
 *
 * 为什么需要它：分层布局把「深度」算得很准，但一个 68 篇、7 层的文档群铺开来
 * 是 8000+ px 宽——**看得出细节，看不出骨架**。收窄到前两层先给结构，需要时再
 * 往上调，是从粗到细读一张图的自然方式。
 *
 * 存在 localStorage 里而不是后端：这是每台设备各自的阅读习惯，跟着 Core 走反而怪。
 */
const DEPTH_KEY = 'prodoc.depth-mode';
const DEPTH_MODES = ['all', '2', '3', '4', '6'];

function loadDepthMode(): string {
  try {
    const raw = localStorage.getItem(DEPTH_KEY);
    return raw && DEPTH_MODES.includes(raw) ? raw : 'all';
  } catch {
    return 'all';
  }
}

const depthMode = ref(loadDepthMode());
const maxDepth = computed(() => (depthMode.value === 'all' ? null : Number(depthMode.value)));

/**
 * 下拉选项：固定几档 + **当前值**。
 *
 * 带上当前值是因为层带标签可直接点（点「L3」= 只看前 4 层），那个数字不一定
 * 落在固定档位里——不在的话 `<select>` 会显示空白，看起来像坏了。
 */
const depthOptions = computed(() => {
  const fixed = depthMode.value === 'all' ? [] : [Number(depthMode.value)];
  return [...new Set([...fixed, 2, 3, 4, 6].filter((n) => Number.isFinite(n)))].sort((a, b) => a - b);
});

/** 点层带：只显示到该层（`limit` 是「层数」，等于 depth+1）。 */
function pickDepth(limit: number): void {
  depthMode.value = String(limit);
}
watch(depthMode, (next) => {
  try {
    localStorage.setItem(DEPTH_KEY, next);
  } catch {
    // 隐私模式 / 存储被禁用：不影响本次会话。
  }
});
watch(
  () => report.value?.issues?.length ?? 0,
  (count, previous) => {
    // 只在**新报告到达**时自动展开，不覆盖用户手动收起。
    if (count > (previous ?? 0) && (report.value?.stats?.errors ?? 0) > 0) {
      reportOpen.value = true;
    }
  },
);
const grouped = computed(() => {
  const issues = report.value?.issues ?? [];
  return [
    { level: 'error' as const, title: '错误', items: issues.filter((item) => item.level === 'error') },
    { level: 'warning' as const, title: '警告', items: issues.filter((item) => item.level === 'warning') },
    { level: 'info' as const, title: '提示', items: issues.filter((item) => item.level === 'info') },
  ].filter((group) => group.items.length > 0);
});

/**
 * 保存：交给 `write.ts`（其中包含与 CLI 一致的三路合并重试）。
 *
 * 这里只负责把"当前打开的根"补进目标——组件的 handler 签名只有
 * `(path, content, base)`，没有 root 概念（它以为自己在单根应用里）。
 */
function handleSave(path: string, content: string, base?: string) {
  const root = prodoc.root;
  if (!root) return Promise.resolve({ ok: false, error: '没有打开的文档根' });
  return saveFile({ teamId: props.teamId, core: core.value, root }, path, content, base);
}

/** 删除：同保存，不做合并（文件都删了没有可合并对象）。 */
function handleDelete(path: string, base?: string) {
  const root = prodoc.root;
  if (!root) return Promise.resolve({ ok: false, error: '没有打开的文档根' });
  return deleteFile({ teamId: props.teamId, core: core.value, root }, path, base);
}

function pick(root: string): void {
  openRoot(props.teamId, core.value, root);
}

function up(): void {
  browse(core.value, parentPath(prodoc.browsePath, browseRoots.value));
}

function enter(path: string): void {
  browse(core.value, path);
}

/**
 * 换目录：退到选择界面。
 *
 * 走 `closeRoot`（而不是自己写 `prodoc.root = null`）——它同时清掉 localStorage
 * 里「上次打开的根」的记忆。只清内存的话，下一次 `prodoc.roots` 事件（切视图
 * 重挂载 / 断线重连 / 手点刷新都会产生）会用那份记忆把用户弹回原目录，
 * 表现就是「点了换目录，一闪又回到刚才那个文档群」。
 */
function back(): void {
  closeRoot(props.teamId, core.value);
}

onMounted(() => {
  // 一进来就拉根列表：overlay 可能是被直接点开的（没经过入口按钮的 prefetch）。
  requestRoots(core.value);
  if (!prodoc.root) browse(core.value, null);
});
</script>

<template>
  <div class="prodoc-panel">
    <!-- ── 已打开：画布 ───────────────────────────────────────── -->
    <template v-if="prodoc.root && entry">
      <header class="prodoc-bar">
        <div class="prodoc-bar-main">
          <PanelIcon name="file-text" size="md" />
          <span class="prodoc-root-wrap" :title="currentRoot?.hint || prodoc.root">
            <strong class="prodoc-root">{{ currentRoot?.label ?? prodoc.root }}</strong>
            <span v-if="currentRoot?.hint && currentRoot.hint !== currentRoot.label" class="prodoc-root-hint">
              {{ currentRoot.hint }}
            </span>
          </span>
          <span class="prodoc-summary">{{ summary }}</span>
        </div>
        <div class="prodoc-bar-actions">
          <label class="prodoc-depth" title="只画到第 N 层：大图先看骨架，需要时再逐层展开">
            <span class="prodoc-depth-label">层级</span>
            <select v-model="depthMode">
              <option value="all">全部</option>
              <option v-for="opt in depthOptions" :key="opt" :value="String(opt)">前 {{ opt }} 层</option>
            </select>
          </label>
          <NeumorphismButton
            variant="glass"
            shape="pill"
            :size="32"
            :disabled="prodoc.checking"
            title="检查文档图：断链 / 重复 id / 环 / 孤立…"
            @click="check(teamId, core)"
          >
            {{ prodoc.checking ? '检查中…' : '检查' }}
          </NeumorphismButton>
          <NeumorphismButton variant="glass" shape="pill" :size="32" @click="refresh(teamId, core)">
            刷新
          </NeumorphismButton>
          <NeumorphismButton variant="glass" shape="pill" :size="32" @click="back">
            换目录
          </NeumorphismButton>
        </div>
      </header>

      <!-- 错误条：比空白画布有用得多 -->
      <div v-if="entry.error" class="prodoc-error">
        <PanelIcon name="circle-info" size="sm" />
        <span>{{ entry.error }}</span>
      </div>

      <!-- 被跳过的文件：不显示出来，用户会以为自己的 link 写错了 -->
      <details v-if="entry.skipped.length" class="prodoc-skipped">
        <summary>{{ entry.skipped.length }} 个文件未载入</summary>
        <ul>
          <li v-for="item in entry.skipped" :key="item.path">
            <code>{{ item.path }}</code> — {{ item.reason }}
          </li>
        </ul>
      </details>

      <!-- 检查结果：有错误时默认展开，纯警告时收起（草稿是常态，不该挡画布） -->
      <section v-if="report" class="prodoc-report">
        <button class="prodoc-report-head" @click="reportOpen = !reportOpen">
          <PanelIcon :name="report.stats?.errors ? 'circle-close' : 'circle-info'" size="sm" />
          <span>
            检查：{{ report.stats?.documents ?? 0 }} 篇 ·
            {{ report.stats?.errors ?? 0 }} 个错误 / {{ report.stats?.warnings ?? 0 }} 条警告
          </span>
          <span class="prodoc-report-toggle">{{ reportOpen ? '收起' : '展开' }}</span>
        </button>
        <div v-if="reportOpen" class="prodoc-report-body">
          <p v-if="grouped.length === 0" class="prodoc-empty">没有发现问题。</p>
          <template v-for="group in grouped" :key="group.level">
            <h4 :class="['prodoc-report-level', `is-${group.level}`]">{{ group.title }}</h4>
            <ul>
              <li v-for="(issue, index) in group.items" :key="`${issue.rule}-${index}`">
                <code>{{ issue.rule }}</code>
                <span>{{ issue.message }}</span>
                <em v-if="issue.file">{{ issue.file }}</em>
              </li>
            </ul>
          </template>
        </div>
      </section>

      <div v-if="entry.loading" class="prodoc-loading">
        <PanelIcon name="circle-info" size="sm" />
        <span>正在扫描文档目录…</span>
      </div>

      <div v-else-if="!entry.error" class="prodoc-canvas">
        <!-- `:key` 按「core + 根」重建实例：渲染器内部有一份**按相对路径**
             存的编辑缓存（`{...files, ...本地编辑}`），换根不重建的话，新根的
             `index.md` 会被旧根同名文件的编辑内容覆盖——画布上出现的是上一个
             目录的文字。 -->
        <DocGraphViewer
          :key="`${core ?? ''}|${prodoc.root ?? ''}`"
          :files="entry.files"
          :max-depth="maxDepth"
          @depth-pick="pickDepth"
          :save-handler="handleSave"
          :delete-handler="handleDelete"
        />
      </div>
    </template>

    <template v-else>
      <section class="prodoc-picker">
        <!-- ① 发现的文档群：自动识别结果（模块在发现范围里找带 `prodoc: true` 的目录） -->
        <div class="prodoc-section">
          <h3 class="prodoc-section-title">
            发现的文档群
            <span class="prodoc-section-note">
              目录里有文档声明了 <code>prodoc: true</code>
            </span>
          </h3>
          <ul class="prodoc-root-list">
            <li v-for="root in roots" :key="root.path">
              <button class="prodoc-root-item" :title="root.hint || root.path" @click="pick(root.path)">
                <PanelIcon name="file-text" size="sm" />
                <span class="prodoc-root-item-text">
                  <span class="prodoc-root-item-label">{{ root.label }}</span>
                  <span v-if="root.hint && root.hint !== root.label" class="prodoc-root-item-hint">
                    {{ root.hint }}
                  </span>
                </span>
              </button>
            </li>
          </ul>
          <p v-if="roots.length === 0" class="prodoc-empty">
            没有发现文档群。在文档的参数区（文件最前面的 <code>---</code> 块）里加一行
            <code>prodoc: true</code>，ProDoc 就会自动把它收进来；发现范围由
            <code>modules.toml</code> 的 <code>[module.config] roots</code> 决定。
          </p>
          <p class="prodoc-hint">
            上限：{{ prodoc.caps.max_files || '—' }} 篇 /
            单文件 {{ Math.round((prodoc.caps.max_file_bytes || 0) / 1024) || '—' }} KB。
          </p>
        </div>

        <!-- ② 浏览目录：找一个新位置 -->
        <div class="prodoc-section">
          <h3 class="prodoc-section-title">
            浏览目录
            <span class="prodoc-section-note">只能进到已配置的根之内</span>
          </h3>
          <div class="prodoc-crumbs">
            <button class="prodoc-crumb" @click="browse(core, null)">根列表</button>
            <template v-for="crumb in crumbs" :key="crumb.path">
              <span class="prodoc-sep">/</span>
              <button class="prodoc-crumb" @click="enter(crumb.path)">{{ crumb.label }}</button>
            </template>
            <NeumorphismButton
              v-if="prodoc.browsePath"
              variant="glass"
              shape="pill"
              :size="32"
              @click="up"
            >
              上一级
            </NeumorphismButton>
            <NeumorphismButton
              v-if="prodoc.browsePath"
              variant="glass"
              shape="pill"
              :size="32"
              @click="pick(prodoc.browsePath as string)"
            >
              用这个目录
            </NeumorphismButton>
          </div>

          <ul class="prodoc-dirs">
            <li v-for="dir in prodoc.browseEntries" :key="dir.path">
              <button class="prodoc-dir" @click="enter(dir.path)">
                <PanelIcon name="folder" size="sm" />
                <span class="prodoc-dir-name">{{ dir.name }}</span>
              </button>
            </li>
            <li v-if="prodoc.browseEntries.length === 0" class="prodoc-empty">
              {{ prodoc.browsePath ? '这一层没有子目录' : '从上面的根选一个，或告诉我你想加哪个目录' }}
            </li>
          </ul>
        </div>
      </section>
    </template>
  </div>
</template>

<style scoped>
.prodoc-panel {
  display: flex;
  flex-direction: column;
  gap: 8px;
  /* 主区视图：宿主容器已是 flex column + overflow hidden，这里只声明
     "占满并让内部滚动"，不重复设 100%（父级高度由 `.extension-view` 定）。 */
  flex: 1 1 auto;
  min-height: 0;
  min-width: 0;
  padding: 12px;
  box-sizing: border-box;
}

.prodoc-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  flex-wrap: wrap;
}

.prodoc-bar-main {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
}

.prodoc-root-wrap {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.35;
}

.prodoc-root {
  font-size: 13px;
}

.prodoc-root-hint {
  font-family: var(--echo-font-mono, monospace);
  font-size: 11px;
  opacity: 0.58;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 52ch;
}

.prodoc-summary {
  font-size: 12px;
  opacity: 0.68;
.prodoc-bar-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* 层级选择器：与胶囊按钮同高，原生 select 去掉默认外观 */
.prodoc-depth {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 0 10px;
  height: 32px;
  border-radius: 999px;
  font-size: 12px;
  color: var(--nm-text-secondary, #666);
  box-shadow: inset 2px 2px 4px var(--nm-shadow-dark, rgba(0, 0, 0, 0.12)),
    inset -2px -2px 4px var(--nm-shadow-light, rgba(255, 255, 255, 0.7));
}

.prodoc-depth-label {
  opacity: 0.7;
}

.prodoc-depth select {
  appearance: none;
  border: none;
  background: transparent;
  color: inherit;
  font: inherit;
  padding: 0 2px;
  cursor: pointer;
  outline: none;
}
}

.prodoc-error {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 10px;
  background: var(--echo-color-danger-soft, rgba(200, 60, 60, 0.12));
  color: var(--echo-color-danger, #b23);
  font-size: 13px;
}

.prodoc-skipped {
  font-size: 12px;
  opacity: 0.85;
}

.prodoc-skipped ul {
  margin: 4px 0 0;
  padding-left: 18px;
  max-height: 120px;
  overflow: auto;
}

.prodoc-report {
  border-radius: 10px;
  background: var(--echo-color-surface-raised, rgba(0, 0, 0, 0.04));
  font-size: 12px;
}

.prodoc-report-head {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  border: none;
  background: none;
  padding: 8px 10px;
  cursor: pointer;
  color: inherit;
  font: inherit;
  text-align: left;
}

.prodoc-report-toggle {
  margin-left: auto;
  opacity: 0.6;
}

.prodoc-report-body {
  padding: 0 10px 10px;
  max-height: 220px;
  overflow: auto;
}

.prodoc-report-body ul {
  margin: 2px 0 6px;
  padding-left: 16px;
}

.prodoc-report-body li {
  display: flex;
  gap: 6px;
  align-items: baseline;
  flex-wrap: wrap;
  line-height: 1.7;
}

.prodoc-report-body em {
  opacity: 0.62;
  font-style: normal;
  font-family: var(--echo-font-mono, monospace);
}

.prodoc-report-level {
  margin: 6px 0 0;
  font-size: 12px;
  font-weight: 600;
}

.prodoc-report-level.is-error {
  color: var(--echo-color-danger, #b23);
}

.prodoc-loading {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
  opacity: 0.8;
}

.prodoc-canvas {
  flex: 1;
  min-height: 0;
  /* ProDoc 的画布要明确的可用高度，否则 auto 布局会塌成 0 */
  position: relative;
}

.prodoc-picker {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow: auto;
}

.prodoc-hint {
  margin: 0;
  font-size: 12px;
  opacity: 0.72;
  line-height: 1.6;
}

.prodoc-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.prodoc-section-title {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}

.prodoc-section-note {
  font-size: 11px;
  font-weight: 400;
  opacity: 0.58;
}

.prodoc-root-list {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.prodoc-root-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  border: none;
  background: none;
  padding: 6px 8px;
  border-radius: 8px;
  cursor: pointer;
  color: inherit;
  font: inherit;
  text-align: left;
}

.prodoc-root-item:hover {
  background: var(--echo-color-surface-raised, rgba(0, 0, 0, 0.05));
}

.prodoc-root-item-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.35;
}

.prodoc-root-item-label {
  font-size: 13px;
}

.prodoc-root-item-hint {
  font-family: var(--echo-font-mono, monospace);
  font-size: 11px;
  opacity: 0.58;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 60ch;
}

.prodoc-crumbs {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-wrap: wrap;
  font-size: 12px;
}

.prodoc-crumb {
  border: none;
  background: none;
  padding: 2px 4px;
  cursor: pointer;
  color: inherit;
  font: inherit;
  text-decoration: underline dotted;
}

.prodoc-sep {
  opacity: 0.5;
}

.prodoc-dirs {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.prodoc-dir {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  border: none;
  background: none;
  padding: 6px 8px;
  border-radius: 8px;
  cursor: pointer;
  color: inherit;
  font: inherit;
  text-align: left;
}

.prodoc-dir:hover {
  background: var(--echo-color-surface-raised, rgba(0, 0, 0, 0.05));
}

.prodoc-dir-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.prodoc-empty {
  font-size: 12px;
  opacity: 0.6;
}
</style>
