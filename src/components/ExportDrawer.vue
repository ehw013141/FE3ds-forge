<script setup lang="ts">
import { ref, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { ipc } from "../services/ipc";
import type { ExportReport, WorkspaceSummary } from "../types/ipc";

const props = defineProps<{
  visible: boolean;
  summary: WorkspaceSummary;
  totalModifiedCount: number;
  defaultOutputDir: string;
}>();

const emit = defineEmits<{
  (e: "close"): void;
}>();

const outputDir = ref<string>(props.defaultOutputDir);
const userCustomizedDir = ref<boolean>(false);

watch(
  () => props.defaultOutputDir,
  (nextDir) => {
    if (!userCustomizedDir.value && nextDir) {
      outputDir.value = nextDir;
    }
  },
);
const exportIps = ref<boolean>(true);
const exportCheat = ref<boolean>(true);
const cheatTitle = ref<string>("");

const exporting = ref(false);
const errorMsg = ref<string | null>(null);
const report = ref<ExportReport | null>(null);

async function pickOutputDir(): Promise<void> {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      title: "选择补丁导出目录（可直接指向 Azahar mods/TitleID 目录）",
    });
    if (typeof selected === "string" && selected.length > 0) {
      outputDir.value = selected;
      userCustomizedDir.value = true;
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  }
}

async function runExport(): Promise<void> {
  exporting.value = true;
  errorMsg.value = null;
  report.value = null;
  try {
    report.value = await ipc.exportWorkspace({
      outputDir: outputDir.value,
      exportIps: exportIps.value,
      exportCheat: exportCheat.value,
      cheatTitle: cheatTitle.value,
    });
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  } finally {
    exporting.value = false;
  }
}
</script>

<template>
  <div v-if="visible" class="backdrop" @click.self="emit('close')">
    <aside class="drawer">
      <header class="drawer-head">
        <div>
          <h2>导出补丁包</h2>
          <p class="sub">自动按 LayeredFS 规范打包 RomFS、ExeFS IPS 与 Azahar 金手指</p>
        </div>
        <button class="close-btn" @click="emit('close')">✕</button>
      </header>

      <div class="drawer-body">
        <div class="summary-box">
          <div class="sum-row">
            <span>当前游戏</span>
            <strong>{{ props.summary.gameTitle }}</strong>
          </div>
          <div class="sum-row">
            <span>待导出修改</span>
            <strong :class="{ ok: props.totalModifiedCount > 0 }">
              {{ props.totalModifiedCount }} 项
            </strong>
          </div>
          <div class="sum-row">
            <span>经验规则</span>
            <strong>
              {{ props.summary.expMode.mode === "vanilla" ? "原版（不生成代码补丁）" : "已启用 ExeFS 补丁" }}
            </strong>
          </div>
        </div>

        <label class="field-group">
          <span>导出目标目录</span>
          <div class="path-row">
            <input v-model="outputDir" type="text" placeholder="选择导出文件夹..." @input="userCustomizedDir = true" />
            <button class="btn-sec" @click="pickOutputDir">浏览</button>
          </div>
        </label>

        <div class="options-group">
          <label class="check-row">
            <input v-model="exportIps" type="checkbox" />
            <span>生成 <code>exefs/code.ips</code>（Azahar / Luma3DS 原生代码补丁）</span>
          </label>
          <label class="check-row">
            <input v-model="exportCheat" type="checkbox" />
            <span>生成 <code>cheats.txt</code>（Azahar 原生金手指格式）</span>
          </label>
        </div>

        <label v-if="exportCheat" class="field-group">
          <span>金手指条目标题</span>
          <input v-model="cheatTitle" type="text" placeholder="留空自动按作品生成（如 FE13 / FE14 Custom EXP）" />
        </label>

        <div v-if="errorMsg" class="alert err">{{ errorMsg }}</div>

        <div v-if="report" class="alert ok">
          <strong>导出成功！共写入 {{ report.written_files.length }} 个文件：</strong>
          <ul>
            <li v-for="file in report.written_files" :key="file">{{ file }}</li>
          </ul>
        </div>
      </div>

      <footer class="drawer-foot">
        <button class="btn-sec" @click="emit('close')">关闭</button>
        <button class="btn-primary" :disabled="exporting || props.totalModifiedCount === 0" @click="runExport">
          {{ exporting ? "正在压缩并写入..." : "立即导出补丁" }}
        </button>
      </footer>
    </aside>
  </div>
</template>

<style scoped>
.backdrop {
  position: fixed;
  inset: 0;
  background: rgba(5, 8, 14, 0.65);
  display: flex;
  justify-content: flex-end;
  z-index: 50;
}

.drawer {
  width: min(460px, 100%);
  height: 100%;
  background: var(--bg-surface);
  border-left: 1px solid var(--border-subtle);
  display: flex;
  flex-direction: column;
}

.drawer-head {
  padding: 18px 20px;
  border-bottom: 1px solid var(--border-subtle);
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 12px;
}

.drawer-head h2 {
  font-size: 16px;
}

.sub {
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-secondary);
}

.close-btn {
  border: none;
  background: transparent;
  color: var(--text-secondary);
  font-size: 16px;
  cursor: pointer;
}

.drawer-body {
  flex: 1;
  overflow-y: auto;
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.summary-box {
  background: var(--bg-elevated);
  border-radius: var(--radius-md);
  padding: 12px 14px;
  display: grid;
  gap: 8px;
}

.sum-row {
  display: flex;
  justify-content: space-between;
  font-size: 12px;
  color: var(--text-secondary);
}

.sum-row strong {
  color: var(--text-primary);
}

.sum-row strong.ok {
  color: var(--accent-success);
}

.field-group {
  display: grid;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
}

.field-group input {
  width: 100%;
  padding: 8px 10px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-subtle);
  background: var(--bg-elevated);
  color: var(--text-primary);
  font-size: 12px;
}

.path-row {
  display: flex;
  gap: 8px;
}

.options-group {
  display: grid;
  gap: 10px;
}

.check-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text-primary);
  cursor: pointer;
}

.alert {
  padding: 10px 12px;
  border-radius: var(--radius-md);
  font-size: 12px;
  line-height: 1.5;
}

.alert.err {
  background: rgba(242, 100, 100, 0.12);
  border: 1px solid rgba(242, 100, 100, 0.35);
  color: var(--accent-danger);
}

.alert.ok {
  background: rgba(56, 201, 139, 0.12);
  border: 1px solid rgba(56, 201, 139, 0.35);
  color: var(--accent-success);
}

.alert.ok ul {
  margin-top: 6px;
  padding-left: 16px;
  word-break: break-all;
}

.drawer-foot {
  padding: 14px 20px;
  border-top: 1px solid var(--border-subtle);
  display: flex;
  justify-content: flex-end;
  gap: 10px;
}

.btn-sec {
  border: 1px solid var(--border-subtle);
  background: var(--bg-elevated);
  color: var(--text-primary);
  border-radius: var(--radius-sm);
  padding: 7px 14px;
  font-size: 12px;
  cursor: pointer;
}

.btn-primary {
  border: none;
  background: var(--accent-primary);
  color: #fff;
  border-radius: var(--radius-sm);
  padding: 7px 16px;
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
}

.btn-primary:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>