<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { ipc } from "../services/ipc";
import type { ExpMultiplier, ExpPatchMode, ExpPatchPreview } from "../types/ipc";

const props = defineProps<{
  hasCodeBin: boolean;
  expSiteOffset: number | null;
  expSiteVaddr: number | null;
  currentMode: ExpPatchMode;
}>();

const emit = defineEmits<{
  (e: "mode-change", mode: ExpPatchMode): void;
}>();

const multiplierOptions: { value: ExpMultiplier; label: string; desc: string }[] = [
  { value: "half", label: "0.5× 减半", desc: "硬核压级玩法（LSR #1）" },
  { value: "one_point_five", label: "1.5× 轻度加速", desc: "略微减少练级耗时" },
  { value: "double", label: "2× 双倍经验", desc: "经典爽快节奏（LSL #1）" },
  { value: "triple", label: "3× 三倍经验", desc: "快速成型阵容" },
  { value: "quadruple", label: "4× 四倍经验", desc: "高速通关（LSL #2）" },
  { value: "quintuple", label: "5× 五倍经验", desc: "极限速通" },
];

const fixedValue = ref<number>(
  props.currentMode.mode === "fixed" ? props.currentMode.value : 25,
);
const selectedPreset = ref<ExpMultiplier>(
  props.currentMode.mode === "multiplier" ? props.currentMode.preset : "double",
);

const applying = ref(false);
const errorMsg = ref<string | null>(null);
const preview = ref<ExpPatchPreview | null>(null);

const canPatch = computed(() => props.hasCodeBin && props.expSiteOffset !== null);

async function applyMode(mode: ExpPatchMode): Promise<void> {
  applying.value = true;
  errorMsg.value = null;
  try {
    const res = await ipc.setExpPatch(mode);
    preview.value = res;
    emit("mode-change", res.mode);
  } catch (err) {
    errorMsg.value = err instanceof Error ? err.message : String(err);
  } finally {
    applying.value = false;
  }
}

function selectVanilla(): void {
  void applyMode({ mode: "vanilla" });
}

function selectMultiplier(preset: ExpMultiplier): void {
  selectedPreset.value = preset;
  void applyMode({ mode: "multiplier", preset });
}

function applyFixed(): void {
  const clamped = Math.max(1, Math.min(100, Math.trunc(Number(fixedValue.value) || 25)));
  fixedValue.value = clamped;
  void applyMode({ mode: "fixed", value: clamped });
}

watch(
  () => props.currentMode,
  (mode) => {
    if (mode.mode === "fixed") {
      fixedValue.value = mode.value;
    } else if (mode.mode === "multiplier") {
      selectedPreset.value = mode.preset;
    }
  },
  { immediate: true },
);
</script>

<template>
  <div class="exp-panel">
    <header class="panel-head">
      <div>
        <h2>战后经验结算规则 (ExeFS)</h2>
        <p class="sub">
          基于 AOB 动态定位 ARM32 经验结算指令，保留原版 100 经验封顶，同时生成 IPS 与 Azahar 金手指。
        </p>
      </div>
      <div class="aob-badge" :class="{ ok: canPatch, warn: !canPatch }">
        <template v-if="canPatch && expSiteVaddr !== null && expSiteOffset !== null">
          AOB 命中 · 偏移 0x{{ expSiteOffset.toString(16).toUpperCase() }} / 虚址 0x{{
            expSiteVaddr.toString(16).toUpperCase().padStart(8, "0")
          }}
        </template>
        <template v-else>未检测到可用 ExeFS/code.bin</template>
      </div>
    </header>

    <div v-if="errorMsg" class="alert err">{{ errorMsg }}</div>

    <div class="cards-grid">
      <!-- 模式 1：原版 -->
      <article
        class="mode-card"
        :class="{ active: currentMode.mode === 'vanilla' }"
      >
        <div class="card-top">
          <h3>原版规则</h3>
          <span class="tag">不生成代码补丁</span>
        </div>
        <p class="card-desc">保持原版等级差经验公式不变，不输出 `code.ips` 或金手指。</p>
        <button
          class="btn-action"
          :disabled="applying || currentMode.mode === 'vanilla'"
          @click="selectVanilla"
        >
          {{ currentMode.mode === "vanilla" ? "当前已启用" : "恢复原版" }}
        </button>
      </article>

      <!-- 模式 2：固定经验 -->
      <article
        class="mode-card"
        :class="{ active: currentMode.mode === 'fixed' }"
      >
        <div class="card-top">
          <h3>固定经验值</h3>
          <span class="tag accent">推荐防凹点</span>
        </div>
        <p class="card-desc">
          每次有效战斗结算固定获得指定经验（1～100），彻底消除低级刷怪或高级无经验问题。
        </p>
        <div class="fixed-row">
          <input
            v-model.number="fixedValue"
            type="range"
            min="1"
            max="100"
            class="slider"
            :disabled="!canPatch"
          />
          <input
            v-model.number="fixedValue"
            type="number"
            min="1"
            max="100"
            class="num-input"
            :disabled="!canPatch"
          />
          <button
            class="btn-action primary"
            :disabled="!canPatch || applying"
            @click="applyFixed"
          >
            应用固定 {{ fixedValue }} EXP
          </button>
        </div>
      </article>

      <!-- 模式 3：倍率缩放 -->
      <article
        class="mode-card wide"
        :class="{ active: currentMode.mode === 'multiplier' }"
      >
        <div class="card-top">
          <h3>经验倍率缩放</h3>
          <span class="tag">单条 ARM32 移位/加法指令</span>
        </div>
        <div class="preset-grid">
          <button
            v-for="opt in multiplierOptions"
            :key="opt.value"
            class="preset-btn"
            :class="{
              selected:
                currentMode.mode === 'multiplier' && currentMode.preset === opt.value,
            }"
            :disabled="!canPatch || applying"
            @click="selectMultiplier(opt.value)"
          >
            <strong>{{ opt.label }}</strong>
            <span>{{ opt.desc }}</span>
          </button>
        </div>
      </article>
    </div>

    <!-- 实时金手指与 IPS 预览 -->
    <section v-if="preview?.active && preview.cheatText" class="preview-card">
      <div class="preview-head">
        <h3>补丁实时预览</h3>
        <span class="meta-pill">IPS 体积 {{ preview.ipsBytes }} B · 覆写 8 B ARM32 指令</span>
      </div>
      <pre class="cheat-box">{{ preview.cheatText }}</pre>
    </section>
  </div>
</template>

<style scoped>
.exp-panel {
  height: 100%;
  overflow-y: auto;
  padding: 22px 26px 36px;
  display: flex;
  flex-direction: column;
  gap: 18px;
}

.panel-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 14px;
  border-bottom: 1px solid var(--border-subtle);
}

.panel-head h2 {
  font-size: 18px;
  font-weight: 600;
}

.sub {
  margin-top: 4px;
  font-size: 12px;
  color: var(--text-secondary);
}

.aob-badge {
  font-size: 12px;
  padding: 5px 11px;
  border-radius: 999px;
  background: var(--bg-elevated);
  color: var(--text-secondary);
}

.aob-badge.ok {
  color: var(--accent-success);
  background: rgba(56, 201, 139, 0.14);
}

.aob-badge.warn {
  color: var(--accent-warning);
  background: rgba(240, 180, 76, 0.14);
}

.alert.err {
  padding: 10px 14px;
  border-radius: var(--radius-md);
  background: rgba(242, 100, 100, 0.12);
  border: 1px solid rgba(242, 100, 100, 0.35);
  color: var(--accent-danger);
  font-size: 13px;
}

.cards-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(280px, 1fr));
  gap: 14px;
}

.mode-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  padding: 18px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.mode-card.wide {
  grid-column: 1 / -1;
}

.mode-card.active {
  border-color: rgba(91, 140, 255, 0.55);
  background: rgba(91, 140, 255, 0.06);
}

.card-top {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}

.card-top h3 {
  font-size: 15px;
  font-weight: 600;
}

.tag {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-elevated);
  padding: 3px 8px;
  border-radius: 999px;
}

.tag.accent {
  color: #a9c7ff;
  background: rgba(91, 140, 255, 0.18);
}

.card-desc {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.fixed-row {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: auto;
}

.slider {
  flex: 1;
  accent-color: var(--accent-primary);
}

.num-input {
  width: 64px;
  padding: 6px 8px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-subtle);
  background: var(--bg-elevated);
  color: var(--text-primary);
  text-align: center;
  font-size: 13px;
}

.btn-action {
  border: 1px solid var(--border-subtle);
  background: var(--bg-elevated);
  color: var(--text-primary);
  border-radius: var(--radius-sm);
  padding: 7px 14px;
  font-size: 12px;
  cursor: pointer;
  width: fit-content;
  margin-top: auto;
}

.btn-action.primary {
  background: var(--accent-primary);
  border-color: transparent;
  color: #fff;
  font-weight: 600;
}

.btn-action:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.preset-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(175px, 1fr));
  gap: 10px;
}

.preset-btn {
  text-align: left;
  border: 1px solid var(--border-subtle);
  background: var(--bg-elevated);
  color: var(--text-primary);
  border-radius: var(--radius-md);
  padding: 10px 12px;
  display: grid;
  gap: 4px;
  cursor: pointer;
}

.preset-btn:hover:not(:disabled) {
  background: var(--bg-hover);
  border-color: var(--border-strong);
}

.preset-btn.selected {
  border-color: var(--accent-primary);
  background: rgba(91, 140, 255, 0.16);
}

.preset-btn strong {
  font-size: 13px;
}

.preset-btn span {
  font-size: 11px;
  color: var(--text-secondary);
}

.preset-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.preview-card {
  background: var(--bg-surface);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  padding: 16px 18px;
  display: grid;
  gap: 10px;
}

.preview-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.preview-head h3 {
  font-size: 14px;
}

.meta-pill {
  font-size: 12px;
  color: var(--accent-success);
  background: rgba(56, 201, 139, 0.14);
  padding: 3px 9px;
  border-radius: 999px;
}

.cheat-box {
  background: var(--bg-base);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-sm);
  padding: 12px 14px;
  font-family: "Cascadia Code", Consolas, monospace;
  font-size: 12px;
  color: #cfe0ff;
  overflow-x: auto;
}
</style>