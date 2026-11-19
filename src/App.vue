<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import ExpRulePanel from "./components/ExpRulePanel.vue";
import ExportDrawer from "./components/ExportDrawer.vue";
import TableWorkbench from "./components/TableWorkbench.vue";
import { ipc } from "./services/ipc";
import type { AppBootstrapInfo, ExpPatchMode, WorkspaceSummary } from "./types/ipc";

const bootstrap = ref<AppBootstrapInfo | null>(null);
const summary = ref<WorkspaceSummary | null>(null);
const activeView = ref<"table" | "exp">("table");
const activeTableId = ref<string>("");
const exportDrawerOpen = ref<boolean>(false);

const manualPath = ref<string>(String.raw`D:\fe13`);
const opening = ref<boolean>(false);
const errorMsg = ref<string | null>(null);
const extracting = ref<boolean>(false);
const noticeMsg = ref<string | null>(null);

const totalModifiedCount = computed<number>(() => {
  if (!summary.value) return 0;
  const tableMods = summary.value.tables.reduce((acc, t) => acc + t.modifiedCount, 0);
  const expMod = summary.value.expMode.mode === "vanilla" ? 0 : 1;
  return tableMods + expMod;
});

async function loadDirectory(dir: string): Promise<void> {
  const cleaned = dir.trim();
  if (!cleaned) {
    errorMsg.value = "请输入或选择一个有效的 3DS 解包游戏目录";
    return;
  }
  opening.value = true;
  errorMsg.value = null;
  noticeMsg.value = null;
  try {
    const loaded = await ipc.openWorkspace(cleaned);
    summary.value = loaded;
    manualPath.value = loaded.rootDir;
    activeView.value = "table";
    activeTableId.value = loaded.tables[0]?.id ?? "";
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  } finally {
    opening.value = false;
  }
}

async function pickDirectory(): Promise<void> {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择 3DS 火焰纹章解包目录（包含 RomFS / ExeFS）",
    });
    if (typeof selected === "string" && selected.length > 0) {
      await loadDirectory(selected);
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  }
}

async function pickRomFile(): Promise<void> {
  try {
    const selected = await open({
      directory: false,
      multiple: false,
      title: "选择已解密的 3DS 火焰纹章 ROM 镜像",
      filters: [
        {
          name: "3DS ROM 镜像 (.3ds / .cci / .cia / .cxi)",
          extensions: ["3ds", "cci", "cia", "cxi"],
        },
      ],
    });
    if (typeof selected === "string" && selected.length > 0) {
      await loadDirectory(selected);
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  }
}

async function extractCurrentRom(): Promise<void> {
  if (!summary.value || summary.value.sourceKind !== "image") return;
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择完整解包输出目录（将生成 RomFS 与 ExeFS）",
    });
    if (typeof selected !== "string" || selected.length === 0) return;

    extracting.value = true;
    errorMsg.value = null;
    noticeMsg.value = null;
    const report = await ipc.extractRom(selected);
    const mb = (report.totalBytes / (1024 * 1024)).toFixed(1);
    noticeMsg.value = `解包完成：RomFS ${report.romfsFiles} 个文件，ExeFS ${report.exefsFiles} 个文件（共 ${mb} MB）→ ${report.outputDir}`;
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  } finally {
    extracting.value = false;
  }
}

function handleTableModified(payload: { tableId: string; modifiedCount: number }): void {
  if (!summary.value) return;
  const target = summary.value.tables.find((t) => t.id === payload.tableId);
  if (target) {
    target.modifiedCount = payload.modifiedCount;
  }
}

function handleExpModeChange(mode: ExpPatchMode): void {
  if (!summary.value) return;
  summary.value.expMode = mode;
}

function selectTable(tableId: string): void {
  activeView.value = "table";
  activeTableId.value = tableId;
}

onMounted(async () => {
  try {
    bootstrap.value = await ipc.getBootstrapInfo();
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  }
});
</script>

<template>
  <div class="app-root">
    <!-- 未加载目录时的启动工作台 -->
    <main v-if="!summary" class="welcome-shell">
      <section class="hero-card">
        <div class="badge">
          {{ bootstrap ? `v${bootstrap.version} · Rust 内核已就绪` : "正在初始化内核..." }}
        </div>
        <h1>FE3DS Forge</h1>
        <p class="subtitle">
          3DS《火焰纹章》三部曲现代化开源可视化修改器 · Schema 驱动 · 无损原地覆写
        </p>

        <div class="open-actions">
          <button class="btn-primary" :disabled="opening" @click="pickRomFile">
            {{ opening ? "正在解析 ROM 数据..." : "打开 ROM 镜像 (.3ds / .cci / .cia)" }}
          </button>
          <button class="btn-secondary full-btn" :disabled="opening" @click="pickDirectory">
            或选择已解包游戏目录（RomFS / ExeFS）
          </button>
        </div>

        <div class="quick-path">
          <input v-model="manualPath" type="text" placeholder="输入解包目录（如 D:\fe13）或 ROM 文件路径（.3ds / .cci / .cia）"
            @keyup.enter="loadDirectory(manualPath)" />
          <button class="btn-secondary" :disabled="opening" @click="loadDirectory(manualPath)">
            快速加载
          </button>
        </div>

        <div v-if="errorMsg" class="alert err">{{ errorMsg }}</div>
      </section>
    </main>

    <!-- 已加载目录时的主工作台 -->
    <div v-else class="studio-shell">
      <header class="topbar">
        <div class="brand">
          <span class="brand-dot" />
          <strong>FE3DS Forge</strong>
          <span class="game-pill">{{ summary.gameTitle }}</span>
          <span v-if="summary.romMeta" class="rom-pill" :title="`TitleID: ${summary.romMeta.titleId}`">
            {{ summary.romMeta.format.toUpperCase() }} ·
            {{ summary.romMeta.productCode || summary.romMeta.titleId }}
          </span>
          <span class="path-text" :title="summary.rootDir">{{ summary.rootDir }}</span>
        </div>

        <div class="topbar-actions">
          <span class="stat-pill">汉化文本 {{ summary.messageCount }} 条</span>
          <span v-if="totalModifiedCount > 0" class="stat-pill dirty">
            待导出修改 {{ totalModifiedCount }} 项
          </span>
          <button class="btn-export" :disabled="totalModifiedCount === 0" @click="exportDrawerOpen = true">
            导出补丁 ({{ totalModifiedCount }})
          </button>
          <button v-if="summary.sourceKind === 'image'" class="btn-secondary" :disabled="extracting"
            @click="extractCurrentRom">
            {{ extracting ? "解包中..." : "解包 ROM" }}
          </button>
          <button class="btn-secondary" :disabled="opening" @click="pickRomFile">
            打开镜像
          </button>
          <button class="btn-secondary" :disabled="opening" @click="pickDirectory">
            打开目录
          </button>
        </div>
      </header>

      <div v-if="noticeMsg" class="studio-banner ok">
        <span>{{ noticeMsg }}</span>
        <button class="banner-close" @click="noticeMsg = null">✕</button>
      </div>
      <div v-if="errorMsg" class="studio-banner err">
        <span>{{ errorMsg }}</span>
        <button class="banner-close" @click="errorMsg = null">✕</button>
      </div>

      <div class="studio-body">
        <aside class="nav-rail">
          <div class="nav-group">
            <div class="nav-heading">RomFS 数据表</div>
            <button v-for="tbl in summary.tables" :key="tbl.id" class="nav-item"
              :class="{ active: activeView === 'table' && activeTableId === tbl.id }" @click="selectTable(tbl.id)">
              <span class="nav-title">{{ tbl.title }}</span>
              <span class="nav-count" :class="{ dirty: tbl.modifiedCount > 0 }">
                {{ tbl.modifiedCount > 0 ? `${tbl.modifiedCount} 改` : tbl.recordCount }}
              </span>
            </button>
          </div>

          <div class="nav-group">
            <div class="nav-heading">ExeFS 逻辑补丁</div>
            <button class="nav-item" :class="{ active: activeView === 'exp' }" @click="activeView = 'exp'">
              <span class="nav-title">战后经验规则</span>
              <span class="nav-count" :class="{ dirty: summary.expMode.mode !== 'vanilla' }">
                {{ summary.expMode.mode === "vanilla" ? "原版" : "已启用" }}
              </span>
            </button>
          </div>
        </aside>

        <main class="main-stage">
          <TableWorkbench v-if="activeView === 'table' && activeTableId" :table-id="activeTableId"
            @modified-change="handleTableModified" />

          <ExpRulePanel v-else :has-code-bin="summary.hasCodeBin" :exp-site-offset="summary.expSiteOffset"
            :exp-site-vaddr="summary.expSiteVaddr" :current-mode="summary.expMode" @mode-change="handleExpModeChange" />
        </main>
      </div>
      <ExportDrawer :visible="exportDrawerOpen" :summary="summary" :total-modified-count="totalModifiedCount"
        :default-output-dir="bootstrap?.defaultOutputDir ?? 'output'" @close="exportDrawerOpen = false" />
    </div>
  </div>
</template>

<style scoped>
.app-root {
  height: 100%;
  display: flex;
  flex-direction: column;
}

.welcome-shell {
  flex: 1;
  display: grid;
  place-items: center;
  padding: 24px;
}

.hero-card {
  width: min(580px, 100%);
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  padding: 32px;
  display: grid;
  gap: 14px;
}

.badge {
  width: fit-content;
  font-size: 12px;
  font-weight: 600;
  color: var(--accent-success);
  background: rgba(56, 201, 139, 0.14);
  padding: 4px 10px;
  border-radius: 999px;
}

h1 {
  font-size: 28px;
  letter-spacing: -0.02em;
}

.subtitle {
  color: var(--text-secondary);
  font-size: 14px;
}

.open-actions {
  margin-top: 6px;
  display: grid;
  gap: 10px;
}

.full-btn {
  width: 100%;
  padding: 9px 14px;
}

.btn-primary {
  width: 100%;
  padding: 11px 16px;
  border: none;
  border-radius: var(--radius-md);
  background: var(--accent-primary);
  color: #fff;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
}

.btn-primary:hover:not(:disabled) {
  filter: brightness(1.08);
}

.btn-primary:disabled,
.btn-secondary:disabled {
  opacity: 0.55;
  cursor: not-allowed;
}

.quick-path {
  display: flex;
  gap: 8px;
}

.quick-path input {
  flex: 1;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  border-radius: var(--radius-md);
  padding: 8px 12px;
  font-size: 13px;
}

.btn-secondary {
  border: 1px solid var(--border-subtle);
  background: var(--bg-elevated);
  color: var(--text-primary);
  border-radius: var(--radius-md);
  padding: 7px 14px;
  font-size: 13px;
  cursor: pointer;
}

.btn-secondary:hover:not(:disabled) {
  background: var(--bg-hover);
  border-color: var(--border-strong);
}

.alert.err {
  padding: 10px 12px;
  border-radius: var(--radius-md);
  background: rgba(242, 100, 100, 0.12);
  border: 1px solid rgba(242, 100, 100, 0.35);
  color: var(--accent-danger);
  font-size: 13px;
}

.studio-shell {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.topbar {
  height: 52px;
  padding: 0 18px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--bg-surface);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  flex-shrink: 0;
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.brand-dot {
  width: 10px;
  height: 10px;
  border-radius: 3px;
  background: var(--accent-primary);
}

.game-pill {
  font-size: 12px;
  padding: 3px 9px;
  border-radius: 999px;
  background: rgba(91, 140, 255, 0.16);
  color: #a9c7ff;
  white-space: nowrap;
}

.path-text {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 320px;
}

.topbar-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.stat-pill {
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-elevated);
  padding: 4px 10px;
  border-radius: 999px;
}

.stat-pill.dirty {
  color: var(--accent-success);
  background: rgba(56, 201, 139, 0.14);
}

.studio-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 220px 1fr;
}

.nav-rail {
  border-right: 1px solid var(--border-subtle);
  background: var(--bg-surface);
  padding: 14px 10px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.nav-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.nav-heading {
  font-size: 11px;
  font-weight: 600;
  color: var(--text-secondary);
  padding: 4px 10px;
  letter-spacing: 0.04em;
}

.nav-item {
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-primary);
  border-radius: var(--radius-sm);
  padding: 9px 10px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-size: 13px;
  cursor: pointer;
}

.nav-item:hover {
  background: var(--bg-elevated);
}

.nav-item.active {
  background: rgba(91, 140, 255, 0.16);
  border-color: rgba(91, 140, 255, 0.45);
}

.nav-count {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-elevated);
  padding: 2px 7px;
  border-radius: 999px;
}

.nav-count.dirty {
  color: var(--accent-success);
  background: rgba(56, 201, 139, 0.14);
}

.main-stage {
  min-width: 0;
  min-height: 0;
  overflow: hidden;
}

.placeholder-panel {
  padding: 32px;
  display: grid;
  gap: 10px;
  color: var(--text-secondary);
}

.placeholder-panel h2 {
  color: var(--text-primary);
  font-size: 18px;
}

.btn-export {
  border: none;
  background: var(--accent-primary);
  color: #fff;
  border-radius: var(--radius-md);
  padding: 7px 14px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}

.btn-export:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.rom-pill {
  font-size: 11px;
  padding: 3px 8px;
  border-radius: 999px;
  background: rgba(56, 201, 139, 0.14);
  color: var(--accent-success);
  white-space: nowrap;
}

.studio-banner {
  padding: 8px 18px;
  font-size: 12px;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  border-bottom: 1px solid var(--border-subtle);
}

.studio-banner.ok {
  background: rgba(56, 201, 139, 0.12);
  color: var(--accent-success);
}

.studio-banner.err {
  background: rgba(242, 100, 100, 0.12);
  color: var(--accent-danger);
}

.banner-close {
  border: none;
  background: transparent;
  color: inherit;
  cursor: pointer;
  font-size: 13px;
}
</style>