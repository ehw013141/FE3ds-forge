<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { ipc } from "../services/ipc";
import type {
  FieldDef,
  FieldKind,
  FieldValue,
  TableDataPayload,
  TableRecord,
} from "../types/ipc";

const props = defineProps<{
  tableId: string;
}>();

const emit = defineEmits<{
  (e: "modified-change", payload: { tableId: string; modifiedCount: number }): void;
}>();

const loading = ref(false);
const saving = ref(false);
const errorMsg = ref<string | null>(null);
const statusMsg = ref<string>("");

const payload = ref<TableDataPayload | null>(null);
const selectedIndex = ref<number>(0);
const searchQuery = ref<string>("");
const onlyModified = ref<boolean>(false);

const batchPresets = [
  { label: "原版", value: 0 },
  { label: "+10", value: 10 },
  { label: "+20", value: 20 },
  { label: "+30", value: 30 },
  { label: "+50", value: 50 },
] as const;

const batchFieldKey = ref<string>("");
const batchDelta = ref<number>(30);
const activeBatchOffset = ref<number | null>(null);

let saveTimer: ReturnType<typeof setTimeout> | null = null;

const portraitMap = ref<Record<string, string | null>>({});
let portraitLoadToken = 0;

function isSafePngDataUrl(url: string | null | undefined): url is string {
  return typeof url === "string" && url.startsWith("data:image/png;base64,");
}

function getPortrait(recordId: string): string | null {
  const url = portraitMap.value[recordId];
  return isSafePngDataUrl(url) ? url : null;
}

async function ensurePortrait(recordId: string): Promise<void> {
  if (!recordId.startsWith("PID_") || recordId in portraitMap.value) {
    return;
  }
  try {
    const dataUrl = await ipc.getPortrait(recordId);
    portraitMap.value[recordId] = isSafePngDataUrl(dataUrl) ? dataUrl : null;
  } catch {
    portraitMap.value[recordId] = null;
  }
}

async function prefetchPortraits(records: TableRecord[], token: number): Promise<void> {
  for (const rec of records) {
    if (token !== portraitLoadToken) return;
    if (!rec.id.startsWith("PID_") || rec.id in portraitMap.value) continue;
    await ensurePortrait(rec.id);
  }
}

function fieldBounds(field: FieldDef): { min: number; max: number } {
  const kind: FieldKind = field.kind;
  let defaultMin = 0;
  let defaultMax = 255;

  switch (kind.type) {
    case "i8":
    case "i8_array":
      defaultMin = -128;
      defaultMax = 127;
      break;
    case "u16":
      defaultMax = 65535;
      break;
    case "i16":
      defaultMin = -32768;
      defaultMax = 32767;
      break;
    case "u32":
      defaultMax = 4294967295;
      break;
    default:
      break;
  }

  return {
    min: field.min ?? defaultMin,
    max: field.max ?? defaultMax,
  };
}

function clamp(val: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, Math.trunc(val)));
}

function valuesEqual(a: FieldValue | undefined, b: FieldValue | undefined): boolean {
  if (!a || !b || a.kind !== b.kind) return false;
  if (a.kind === "int" && b.kind === "int") {
    return a.value === b.value;
  }
  if (a.kind === "int_array" && b.kind === "int_array") {
    return a.value.length === b.value.length && a.value.every((v, i) => v === b.value[i]);
  }
  return false;
}

function isRecordModified(rec: TableRecord): boolean {
  const orig = payload.value?.originalRecords[rec.index];
  if (!orig || !payload.value) return false;
  return payload.value.schema.fields.some(
    (f) => !valuesEqual(rec.values[f.key], orig.values[f.key]),
  );
}

const filteredRecords = computed<TableRecord[]>(() => {
  if (!payload.value) return [];
  const q = searchQuery.value.trim().toLowerCase();
  return payload.value.records.filter((rec) => {
    if (onlyModified.value && !isRecordModified(rec)) {
      return false;
    }
    if (!q) return true;
    return (
      rec.display_name.toLowerCase().includes(q) ||
      rec.id.toLowerCase().includes(q) ||
      String(rec.index).includes(q)
    );
  });
});

const selectedRecord = computed<TableRecord | null>(() => {
  if (!payload.value) return null;
  return payload.value.records[selectedIndex.value] ?? null;
});

const selectedOriginal = computed<TableRecord | null>(() => {
  if (!payload.value) return null;
  return payload.value.originalRecords[selectedIndex.value] ?? null;
});

const batchableFields = computed<FieldDef[]>(() => {
  if (!payload.value) return [];
  return payload.value.schema.fields;
});

const activeBatchField = computed<FieldDef | null>(() => {
  return batchableFields.value.find((f) => f.key === batchFieldKey.value) ?? null;
});

async function loadTable(id: string): Promise<void> {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
  loading.value = true;
  errorMsg.value = null;
  statusMsg.value = "";
  try {
    const data = await ipc.getTableData(id);
    payload.value = data;
    selectedIndex.value = data.records.length > 0 ? 0 : -1;
    searchQuery.value = "";
    onlyModified.value = false;

    const preferred =
      data.schema.fields.find((f) => f.kind.type === "ciphered_u8_array") ??
      data.schema.fields[0];
    batchFieldKey.value = preferred ? preferred.key : "";
    activeBatchOffset.value = null;
    portraitLoadToken += 1;
    const token = portraitLoadToken;
    if (data.schema.entry_prefix === "PID_") {
      void prefetchPortraits(data.records, token);
    }
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  } finally {
    loading.value = false;
  }
}

async function persistRecords(targetId: string, snapshot: TableRecord[]): Promise<void> {
  saving.value = true;
  errorMsg.value = null;
  try {
    const modifiedCount = await ipc.saveTableRecords(targetId, snapshot);
    if (payload.value && payload.value.schema.id === targetId) {
      payload.value.modifiedCount = modifiedCount;
    }
    emit("modified-change", { tableId: targetId, modifiedCount });
    statusMsg.value = `已同步至工作区（当前已改动 ${modifiedCount} 项）`;
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  } finally {
    saving.value = false;
  }
}

function scheduleSave(immediate = false): void {
  if (!payload.value) return;
  const targetId = payload.value.schema.id;
  const snapshot: TableRecord[] = JSON.parse(JSON.stringify(payload.value.records));

  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }

  if (immediate) {
    void persistRecords(targetId, snapshot);
  } else {
    saveTimer = setTimeout(() => {
      saveTimer = null;
      void persistRecords(targetId, snapshot);
    }, 180);
  }
}

function getArrayValues(rec: TableRecord, key: string): number[] {
  const val = rec.values[key];
  return val?.kind === "int_array" ? val.value : [];
}

function getScalarValue(rec: TableRecord, key: string): number {
  const val = rec.values[key];
  return val?.kind === "int" ? val.value : 0;
}

function sumArray(nums: number[]): number {
  return nums.reduce((acc, n) => acc + n, 0);
}

function formatSigned(diff: number): string {
  if (diff > 0) return `+${diff}`;
  return String(diff);
}

function updateArrayStat(field: FieldDef, statIdx: number, rawInput: string): void {
  if (!selectedRecord.value) return;
  const parsed = Number(rawInput);
  if (rawInput.trim() === "" || rawInput.trim() === "-" || Number.isNaN(parsed)) {
    return;
  }
  const { min, max } = fieldBounds(field);
  const current = selectedRecord.value.values[field.key];
  if (!current || current.kind !== "int_array") return;

  const next = [...current.value];
  next[statIdx] = clamp(parsed, min, max);
  selectedRecord.value.values[field.key] = { kind: "int_array", value: next };
  activeBatchOffset.value = null;
  scheduleSave(false);
}

function updateScalarField(field: FieldDef, rawInput: string): void {
  if (!selectedRecord.value) return;
  const parsed = Number(rawInput);
  if (rawInput.trim() === "" || rawInput.trim() === "-" || Number.isNaN(parsed)) {
    return;
  }
  const { min, max } = fieldBounds(field);
  selectedRecord.value.values[field.key] = {
    kind: "int",
    value: clamp(parsed, min, max),
  };
  activeBatchOffset.value = null;
  scheduleSave(false);
}

const isItemTable = computed(() => payload.value?.schema.entry_prefix === "IID_");
const skipZeroOriginal = ref(false);

watch(
  () => payload.value?.schema.id,
  () => {
    skipZeroOriginal.value = payload.value?.schema.entry_prefix === "IID_";
  },
  { immediate: true },
);

function isPlaceholderRecord(rec: TableRecord): boolean {
  if (!isItemTable.value) return false;
  return (
    rec.id === "IID_無し" ||
    rec.id === "IID_不明" ||
    rec.id.startsWith("IID_REFINE")
  );
}

function isZeroOriginal(orig: TableRecord | undefined, fieldKey: string): boolean {
  const val = orig?.values[fieldKey];
  if (!val) return true;
  if (val.kind === "int") return val.value === 0;
  return val.value.every((n) => n === 0);
}

const batchTargets = computed<TableRecord[]>(() => {
  if (!payload.value || !activeBatchField.value) return [];
  const fieldKey = activeBatchField.value.key;
  return filteredRecords.value.filter((rec) => {
    if (isPlaceholderRecord(rec)) return false;
    if (skipZeroOriginal.value) {
      const orig = payload.value!.originalRecords[rec.index];
      if (isZeroOriginal(orig, fieldKey)) return false;
    }
    return true;
  });
});

const infiniteDurabilityTargets = computed<TableRecord[]>(() => {
  if (!payload.value || !isItemTable.value || activeBatchField.value?.key !== "uses") {
    return [];
  }
  return filteredRecords.value.filter((rec) => {
    if (isPlaceholderRecord(rec)) return false;
    const orig = payload.value!.originalRecords[rec.index];
    const origUses = orig?.values["uses"];
    const origMight = orig?.values["might"];
    return (
      origUses?.kind === "int" &&
      origUses.value > 0 &&
      origMight?.kind === "int" &&
      origMight.value > 0
    );
  });
});

function setWeaponsInfiniteDurability(): void {
  const targets = infiniteDurabilityTargets.value;
  if (targets.length === 0) return;
  for (const rec of targets) {
    rec.values["uses"] = { kind: "int", value: 0 };
  }
  activeBatchOffset.value = null;
  scheduleSave(true);
}

function applyBatchOffset(offset: number): void {
  if (!payload.value || !activeBatchField.value) return;
  if (!Number.isFinite(offset)) return;
  const normalized = Math.trunc(offset);
  const field = activeBatchField.value;
  const { min, max } = fieldBounds(field);
  const targets = normalized === 0 ? filteredRecords.value : batchTargets.value;
  if (targets.length === 0) return;

  for (const rec of targets) {
    if (isPlaceholderRecord(rec)) continue;
    const orig = payload.value.originalRecords[rec.index];
    const baseVal = orig?.values[field.key];
    if (!baseVal) continue;
    if (baseVal.kind === "int_array") {
      rec.values[field.key] = {
        kind: "int_array",
        value: baseVal.value.map((n) => clamp(n + normalized, min, max)),
      };
    } else if (baseVal.kind === "int") {
      rec.values[field.key] = {
        kind: "int",
        value: clamp(baseVal.value + normalized, min, max),
      };
    }
  }
  activeBatchOffset.value = normalized;
  scheduleSave(true);
}

function resetSelectedRecord(): void {
  if (!selectedRecord.value || !selectedOriginal.value) return;
  selectedRecord.value.values = JSON.parse(JSON.stringify(selectedOriginal.value.values));
  scheduleSave(true);
}

function resetAllRecords(): void {
  if (!payload.value) return;
  payload.value.records = JSON.parse(JSON.stringify(payload.value.originalRecords));
  activeBatchOffset.value = null;
  activeBatchOffset.value = 0;
  scheduleSave(true);
}

function canRenderSlider(field: FieldDef): boolean {
  const { min, max } = fieldBounds(field);
  return max - min <= 300;
}

watch(
  () => props.tableId,
  (id) => {
    if (id) {
      void loadTable(id);
    }
  },
  { immediate: true },
);

watch(
  selectedRecord,
  (rec) => {
    if (rec) {
      void ensurePortrait(rec.id);
    }
  },
  { immediate: true },
);

watch(batchFieldKey, () => {
  activeBatchOffset.value = null;
});

onBeforeUnmount(() => {
  if (saveTimer) {
    clearTimeout(saveTimer);
    saveTimer = null;
  }
});
</script>

<template>
  <div class="workbench">
    <div v-if="loading" class="state-box">正在加载数据表...</div>
    <div v-else-if="errorMsg" class="state-box err">操作异常：{{ errorMsg }}</div>

    <template v-else-if="payload">
      <!-- 顶部批量规则工具栏 -->
      <header class="toolbar">
        <div class="toolbar-title">
          <h2>{{ payload.schema.title }}</h2>
          <span class="meta-pill">{{ payload.schema.target_file }}</span>
          <span v-if="payload.modifiedCount > 0" class="meta-pill modified">
            已修改 {{ payload.modifiedCount }} / {{ payload.records.length }}
          </span>
        </div>

        <div class="batch-controls">
          <span class="batch-label">批量法则</span>
          <select v-model="batchFieldKey" class="compact-select">
            <option v-for="f in batchableFields" :key="f.key" :value="f.key">
              {{ f.label }}
            </option>
          </select>

          <div class="preset-segment" role="radiogroup" aria-label="全局基准偏移单选">
            <button v-for="preset in batchPresets" :key="preset.value" class="btn-chip seg-item"
              :class="{ accent: activeBatchOffset === preset.value }" :aria-pressed="activeBatchOffset === preset.value"
              @click="applyBatchOffset(preset.value)">
              {{ preset.label }}
            </button>
          </div>

          <div class="custom-delta">
            <input v-model.number="batchDelta" type="number" class="delta-input" title="基于原版的自定义偏移值" />
            <button class="btn-chip" :disabled="batchTargets.length === 0"
              :title="isItemTable ? '已自动跳过占位槽' + (skipZeroOriginal ? '与原版为 0 项' : '') : ''"
              @click="applyBatchOffset(batchDelta)">
              设为原版{{
                Number.isFinite(batchDelta)
                  ? batchDelta >= 0
                    ? `+${batchDelta}`
                    : batchDelta
                  : "+0"
              }}（{{ batchTargets.length }}项）
            </button>
          </div>

          <label v-if="isItemTable" class="filter-toggle" title="开启后，批量偏移自动跳过该字段原版为 0 的条目（防止误改伤药、盾牌等非武器项）">
            <input v-model="skipZeroOriginal" type="checkbox" />
            <span>跳过原版为0</span>
          </label>

          <button v-if="infiniteDurabilityTargets.length > 0" class="btn-chip accent"
            title="仅将原版威力 > 0 且耐久 > 0 的武器耐久设为 0（无限），不误伤伤药等消耗品" @click="setWeaponsInfiniteDurability">
            武器无限耐久（{{ infiniteDurabilityTargets.length }}项）
          </button>

          <button class="btn-chip danger" :disabled="payload.modifiedCount === 0" @click="resetAllRecords">
            全表还原
          </button>
        </div>
      </header>

      <!-- 主体双栏：左侧检索列表 + 右侧数值编辑器 -->
      <div class="split-body">
        <aside class="record-list-pane">
          <div class="search-bar">
            <input v-model="searchQuery" type="text" placeholder="搜索译名 / 内部 ID..." class="search-input" />
            <label class="filter-toggle">
              <input v-model="onlyModified" type="checkbox" />
              <span>仅看已改</span>
            </label>
          </div>

          <div class="record-scroll">
            <button v-for="rec in filteredRecords" :key="rec.id" class="record-item" :class="{
              active: rec.index === selectedIndex,
              dirty: isRecordModified(rec),
            }" @click="selectedIndex = rec.index">
              <img v-if="getPortrait(rec.id)" :src="getPortrait(rec.id)!" :alt="rec.display_name" class="avatar-sm" />
              <span v-else class="rec-idx">#{{ String(rec.index).padStart(2, "0") }}</span>
              <span class="rec-main">
                <span class="rec-name">{{ rec.display_name }}</span>
                <span class="rec-id">{{ rec.id }}</span>
              </span>
              <span v-if="isRecordModified(rec)" class="dirty-dot" title="已修改" />
            </button>

            <div v-if="filteredRecords.length === 0" class="empty-list">
              没有匹配的数据条目
            </div>
          </div>
        </aside>

        <section class="editor-pane">
          <template v-if="selectedRecord && selectedOriginal">
            <div class="record-header">
              <div class="record-head-main">
                <img v-if="getPortrait(selectedRecord.id)" :src="getPortrait(selectedRecord.id)!"
                  :alt="selectedRecord.display_name" class="avatar-lg" />
                <div>
                  <h3>{{ selectedRecord.display_name }}</h3>
                  <p class="record-sub">
                    <code>{{ selectedRecord.id }}</code>
                    <span>·</span>
                    <span>序号 #{{ selectedRecord.index }}</span>
                    <span>·</span>
                    <span>偏移 0x{{ selectedRecord.offset.toString(16).toUpperCase() }}</span>
                  </p>
                </div>
              </div>
              <div class="header-actions">
                <span v-if="saving" class="sync-tag">同步中...</span>
                <span v-else-if="statusMsg" class="sync-tag ok">{{ statusMsg }}</span>
                <button class="btn-chip" :disabled="!isRecordModified(selectedRecord)" @click="resetSelectedRecord">
                  还原当前项
                </button>
              </div>
            </div>

            <div class="fields-stack">
              <article v-for="field in payload.schema.fields" :key="field.key" class="field-card">
                <div class="field-head">
                  <div>
                    <h4>{{ field.label }}</h4>
                    <span class="field-meta">
                      +0x{{ field.offset.toString(16).toUpperCase().padStart(2, "0") }}
                      · {{ field.kind.type }}
                    </span>
                  </div>

                  <div v-if="selectedRecord.values[field.key]?.kind === 'int_array'" class="sum-badge">
                    总和
                    <strong>{{ sumArray(getArrayValues(selectedRecord, field.key)) }}</strong>
                    <span v-if="
                      sumArray(getArrayValues(selectedRecord, field.key)) !==
                      sumArray(getArrayValues(selectedOriginal, field.key))
                    " class="delta-tag">
                      ({{
                        formatSigned(
                          sumArray(getArrayValues(selectedRecord, field.key)) -
                          sumArray(getArrayValues(selectedOriginal, field.key)),
                        )
                      }})
                    </span>
                  </div>
                </div>

                <!-- 8 维数组字段（成长率 / 修正 / 上限） -->
                <div v-if="selectedRecord.values[field.key]?.kind === 'int_array'" class="stat-grid">
                  <div v-for="(val, idx) in getArrayValues(selectedRecord, field.key)" :key="idx" class="stat-row"
                    :class="{
                      changed: val !== getArrayValues(selectedOriginal, field.key)[idx],
                    }">
                    <span class="stat-name">
                      {{ field.labels?.[idx] ?? `#${idx + 1}` }}
                    </span>

                    <div v-if="canRenderSlider(field)" class="slider-wrap">
                      <input type="range" class="stat-slider" :min="fieldBounds(field).min"
                        :max="fieldBounds(field).max" :value="val" @input="
                          updateArrayStat(
                            field,
                            idx,
                            ($event.target as HTMLInputElement).value,
                          )
                          " />
                    </div>

                    <input type="number" class="stat-num" :min="fieldBounds(field).min" :max="fieldBounds(field).max"
                      :value="val" @input="
                        updateArrayStat(
                          field,
                          idx,
                          ($event.target as HTMLInputElement).value,
                        )
                        " />

                    <span class="orig-ref">
                      原 {{ getArrayValues(selectedOriginal, field.key)[idx] }}
                      <em v-if="val !== getArrayValues(selectedOriginal, field.key)[idx]" class="diff-pill">
                        {{
                          formatSigned(
                            val - getArrayValues(selectedOriginal, field.key)[idx],
                          )
                        }}
                      </em>
                    </span>
                  </div>
                </div>

                <!-- 标量整型字段（供后续武器/道具表自动渲染） -->
                <div v-else class="scalar-row">
                  <input v-if="canRenderSlider(field)" type="range" class="stat-slider" :min="fieldBounds(field).min"
                    :max="fieldBounds(field).max" :value="getScalarValue(selectedRecord, field.key)" @input="
                      updateScalarField(field, ($event.target as HTMLInputElement).value)
                      " />
                  <input type="number" class="stat-num wide" :min="fieldBounds(field).min" :max="fieldBounds(field).max"
                    :value="getScalarValue(selectedRecord, field.key)" @input="
                      updateScalarField(field, ($event.target as HTMLInputElement).value)
                      " />
                  <span class="orig-ref">
                    原版 {{ getScalarValue(selectedOriginal, field.key) }}
                  </span>
                </div>
              </article>
            </div>
          </template>
        </section>
      </div>
    </template>
  </div>
</template>

<style scoped>
.workbench {
  height: 100%;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.state-box {
  margin: 24px;
  padding: 18px 20px;
  border-radius: var(--radius-md);
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  color: var(--text-secondary);
}

.state-box.err {
  color: var(--accent-danger);
  border-color: rgba(242, 100, 100, 0.35);
}

.toolbar {
  padding: 14px 20px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--bg-surface);
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}

.toolbar-title {
  display: flex;
  align-items: center;
  gap: 10px;
}

.toolbar-title h2 {
  font-size: 16px;
  font-weight: 600;
}

.meta-pill {
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-elevated);
  padding: 3px 9px;
  border-radius: 999px;
}

.meta-pill.modified {
  color: var(--accent-success);
  background: rgba(56, 201, 139, 0.14);
}

.batch-controls {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.batch-label {
  font-size: 12px;
  color: var(--text-secondary);
}

.compact-select,
.delta-input,
.search-input,
.stat-num {
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  color: var(--text-primary);
  border-radius: var(--radius-sm);
  padding: 5px 8px;
  font-size: 12px;
}

.delta-input {
  width: 62px;
  text-align: center;
}

.custom-delta {
  display: flex;
  align-items: center;
  gap: 4px;
}

.btn-chip {
  border: 1px solid var(--border-subtle);
  background: var(--bg-elevated);
  color: var(--text-primary);
  border-radius: var(--radius-sm);
  padding: 5px 10px;
  font-size: 12px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
}

.btn-chip:hover:not(:disabled) {
  background: var(--bg-hover);
  border-color: var(--border-strong);
}

.btn-chip:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.btn-chip.accent {
  background: rgba(91, 140, 255, 0.18);
  border-color: rgba(91, 140, 255, 0.45);
  color: #9ec0ff;
}

.btn-chip.danger {
  color: #ff9c9c;
  border-color: rgba(242, 100, 100, 0.35);
}

.split-body {
  flex: 1;
  min-height: 0;
  display: grid;
  grid-template-columns: 270px 1fr;
}

.record-list-pane {
  border-right: 1px solid var(--border-subtle);
  background: rgba(23, 28, 38, 0.65);
  display: flex;
  flex-direction: column;
  min-height: 0;
}

.search-bar {
  padding: 12px;
  border-bottom: 1px solid var(--border-subtle);
  display: grid;
  gap: 8px;
}

.search-input {
  width: 100%;
  padding: 7px 10px;
}

.filter-toggle {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--text-secondary);
  cursor: pointer;
}

.record-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.record-item {
  width: 100%;
  text-align: left;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text-primary);
  border-radius: var(--radius-sm);
  padding: 8px 10px;
  display: flex;
  align-items: center;
  gap: 10px;
  cursor: pointer;
}

.record-item:hover {
  background: var(--bg-elevated);
}

.record-item.active {
  background: rgba(91, 140, 255, 0.16);
  border-color: rgba(91, 140, 255, 0.45);
}

.rec-idx {
  font-size: 11px;
  color: var(--text-secondary);
  font-variant-numeric: tabular-nums;
}

.rec-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}

.rec-name {
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.rec-id {
  font-size: 11px;
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.dirty-dot {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--accent-success);
  flex-shrink: 0;
}

.empty-list {
  padding: 24px 12px;
  text-align: center;
  font-size: 12px;
  color: var(--text-secondary);
}

.editor-pane {
  overflow-y: auto;
  padding: 20px 24px 32px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.record-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 16px;
  padding-bottom: 14px;
  border-bottom: 1px solid var(--border-subtle);
}

.record-header h3 {
  font-size: 20px;
  font-weight: 600;
}

.record-sub {
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-secondary);
  display: flex;
  gap: 8px;
  align-items: center;
}

.header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.sync-tag {
  font-size: 12px;
  color: var(--text-secondary);
}

.sync-tag.ok {
  color: var(--accent-success);
}

.fields-stack {
  display: grid;
  gap: 16px;
}

.field-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  padding: 16px 18px;
  display: grid;
  gap: 14px;
}

.field-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  gap: 12px;
}

.field-head h4 {
  font-size: 14px;
  font-weight: 600;
}

.field-meta {
  font-size: 11px;
  color: var(--text-secondary);
}

.sum-badge {
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-elevated);
  padding: 4px 10px;
  border-radius: 999px;
}

.sum-badge strong {
  color: var(--text-primary);
  margin-left: 4px;
}

.delta-tag {
  color: var(--accent-success);
  margin-left: 4px;
}

.stat-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: 10px 20px;
}

.stat-row {
  display: grid;
  grid-template-columns: 38px 1fr 62px 86px;
  align-items: center;
  gap: 10px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  background: rgba(255, 255, 255, 0.02);
}

.stat-row.changed {
  background: rgba(56, 201, 139, 0.08);
}

.stat-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary);
}

.slider-wrap {
  display: flex;
  align-items: center;
}

.stat-slider {
  width: 100%;
  accent-color: var(--accent-primary);
  cursor: pointer;
}

.stat-num {
  width: 62px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.stat-num.wide {
  width: 110px;
}

.orig-ref {
  font-size: 11px;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  font-variant-numeric: tabular-nums;
}

.diff-pill {
  font-style: normal;
  font-weight: 600;
  color: var(--accent-success);
}

.scalar-row {
  display: flex;
  align-items: center;
  gap: 12px;
}

.avatar-sm {
  width: 30px;
  height: 30px;
  border-radius: 6px;
  object-fit: cover;
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  flex-shrink: 0;
}

.record-head-main {
  display: flex;
  align-items: center;
  gap: 14px;
}

.avatar-lg {
  width: 64px;
  height: 64px;
  border-radius: 10px;
  object-fit: cover;
  background: var(--bg-base);
  border: 1px solid var(--border-strong);
  flex-shrink: 0;
}

.preset-segment {
  display: inline-flex;
  align-items: center;
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
  padding: 2px;
  gap: 2px;
}

.seg-item {
  border-color: transparent;
  background: transparent;
  padding: 4px 9px;
}
</style>