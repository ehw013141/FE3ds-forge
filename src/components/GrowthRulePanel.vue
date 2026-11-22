<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type {
  GrowthRulesConfig,
  GrowthRulesPreview,
  SupportSpeedPreset,
  WeaponExpPreset,
} from "../types/ipc";

const props = defineProps<{
  gameId: string;
  hasCodeBin: boolean;
  currentConfig: GrowthRulesConfig;
}>();

const emit = defineEmits<{
  (e: "config-change", config: GrowthRulesConfig): void;
}>();

const preview = ref<GrowthRulesPreview | null>(null);
const loading = ref(false);
const errorMsg = ref<string | null>(null);

const supportOptions: { id: SupportSpeedPreset; title: string; desc: string }[] = [
  { id: "vanilla", title: "原版默认", desc: "保持原版支援升级点数" },
  { id: "fast2x", title: "2 倍速", desc: "各级所需支援点数约减半" },
  { id: "fast4x", title: "4 倍速", desc: "各级所需支援点数压至约 1/4" },
  { id: "instant", title: "1 战极速", desc: "压至最低递增门槛 (1 / 2 / 3 / 4)" },
];

const weaponExpOptions = computed<{ id: WeaponExpPreset; title: string; desc: string }[]>(() => {
  if (props.gameId === "fe15") {
    return [
      { id: "vanilla", title: "原版默认", desc: "保持原版武器战技 SP 需求" },
      { id: "fast2x", title: "2 倍习得", desc: "全部武器战技 SP 需求减半" },
      { id: "fast5x", title: "5 倍习得", desc: "全部武器战技 SP 压至 1/5" },
      { id: "fast10x", title: "10 倍习得", desc: "全部武器战技 SP 压至 1/10" },
      { id: "instant", title: "1 战习得", desc: "所有武器战技 SP 统一设为 1" },
    ];
  }

  if (props.gameId === "fe14") {
    return [
      { id: "vanilla", title: "原版默认", desc: "保持原版单次战斗/用杖熟练度 (+2)" },
      { id: "fast2x", title: "2 倍熟练", desc: "战斗与用杖熟练度 ×2（单次 +4）" },
      { id: "fast5x", title: "5 倍熟练", desc: "战斗与用杖熟练度 ×5（单次 +10）" },
      { id: "fast10x", title: "8 倍熟练", desc: "战斗与用杖熟练度 ×8（单次 +16）" },
      { id: "instant", title: "一击满级", desc: "单次使用直达当前职业上限（自动封顶）" },
    ];
  }
  return [
    { id: "vanilla", title: "原版默认", desc: "保持原版单次攻击/用杖熟练度" },
    { id: "fast2x", title: "2 倍熟练", desc: "单次获取武器熟练度 ×2" },
    { id: "fast5x", title: "5 倍熟练", desc: "单次获取武器熟练度 ×5" },
    { id: "fast10x", title: "10 倍熟练", desc: "单次获取武器熟练度 ×10" },
    { id: "instant", title: "一击满级", desc: "单次使用直接满级（A 级上限 91）" },
  ];
});

const supportMeta = computed(() => {
  switch (props.gameId) {
    case "fe13":
      return {
        file: "RomFS/data/person/static.bin.lz",
        unit: "个支援槽",
        note: "按比例缩放全员 C / B / A / S 支援阈值，自动保留非结婚 S 锁（99）。",
      };
    case "fe14":
      return {
        file: "RomFS/GameData/GameData.bin.lz",
        unit: "组支援关系",
        note: "动态定位全员支援表缩放 C / B / A / S 阈值，自动保留不可结婚 S 锁（255）。",
      };
    default:
      return {
        file: "RomFS/Data/Reliance.bin.lz",
        unit: "条支援曲线",
        note: "修改全员共用的慢 / 中 / 快三档支援成长曲线（C / B / A）。",
      };
  }
});

const weaponMeta = computed(() => {
  if (props.gameId === "fe13") {
    return {
      title: "武器熟练度加速 (WExp)",
      file: "RomFS/data/GameData.bin.lz",
      unit: "件武器与魔杖",
      supported: true,
      note: "放大 ItemData 单次攻击/用杖熟练度 (+0x1D)，自动跳过消耗品，不破坏初始武器等级。",
    };
  }
  if (props.gameId === "fe15") {
    return {
      title: "武器战技熟练度加速 (SP)",
      file: "RomFS/Data/Skill.bin.lz",
      unit: "项武器战技",
      supported: true,
      note: "《回声》无武器等级限制，此处降低全部武器战技的习得 SP 门槛 (+0x0D)。",
    };
  }
  return {
    title: "武器熟练度加速 (ExeFS 补丁)",
    file: "ExeFS/code.ips & cheats.txt",
    unit: "处代码位点（攻/守/杖）",
    supported: props.hasCodeBin,
    note: props.hasCodeBin
      ? "通过 3 处无分支偏移 AOB 精准放大攻方、守方及用杖熟练度，不影响敌兵初始化且自带职业上限封顶，导出时自动合并至 code.ips 与 cheats.txt。"
      : "当前目录缺少 ExeFS/code.bin，无法生成 FE14 武器熟练度代码补丁（请直接加载 .cci/.3ds/.cia 或完整解包目录）。",
  };
});

async function applyConfig(next: GrowthRulesConfig) {
  loading.value = true;
  errorMsg.value = null;
  try {
    const res = await invoke<GrowthRulesPreview>("set_growth_rules", { config: next });
    preview.value = res;
    emit("config-change", res.config);
  } catch (err) {
    errorMsg.value = String(err);
  } finally {
    loading.value = false;
  }
}

function selectSupport(preset: SupportSpeedPreset) {
  applyConfig({ ...props.currentConfig, supportSpeed: preset });
}

function selectWeaponExp(preset: WeaponExpPreset) {
  if (!weaponMeta.value.supported) return;
  applyConfig({ ...props.currentConfig, weaponExp: preset });
}

function resetBoth() {
  applyConfig({ supportSpeed: "vanilla", weaponExp: "vanilla" });
}

function fmtSeq(arr: number[]): string {
  return arr.map((v) => (v === 99 || v === 255 ? "锁" : String(v))).join(" / ");
}

onMounted(() => {
  applyConfig(props.currentConfig);
});

watch(
  () => props.gameId,
  () => {
    applyConfig(props.currentConfig);
  }
);
</script>

<template>
  <div class="growth-panel">
    <header class="panel-header">
      <div>
        <h2>支援度与熟练度加速</h2>
        <p class="panel-sub">支援度全系列走纯 RomFS · FE13/15 熟练度走 RomFS · FE14 熟练度走 ExeFS（IPS / 金手指）</p>
      </div>
      <button class="btn-reset" :disabled="loading ||
        (currentConfig.supportSpeed === 'vanilla' && currentConfig.weaponExp === 'vanilla')
        " @click="resetBoth">
        全部还原原版
      </button>
    </header>

    <div v-if="errorMsg" class="panel-err">{{ errorMsg }}</div>

    <!-- 卡片 1：支援度加速 -->
    <section class="rule-card">
      <div class="card-head">
        <div>
          <h3>支援度快速提升</h3>
          <code class="file-tag">{{ supportMeta.file }}</code>
        </div>
        <span class="status-pill" :class="{ active: currentConfig.supportSpeed !== 'vanilla' }">
          {{ currentConfig.supportSpeed === "vanilla" ? "原版" : "已启用" }}
        </span>
      </div>
      <p class="card-note">{{ supportMeta.note }}</p>

      <div class="preset-grid">
        <button v-for="opt in supportOptions" :key="opt.id" class="preset-btn"
          :class="{ selected: currentConfig.supportSpeed === opt.id }" :disabled="loading"
          @click="selectSupport(opt.id)">
          <span class="preset-title">{{ opt.title }}</span>
          <span class="preset-desc">{{ opt.desc }}</span>
        </button>
      </div>

      <div v-if="preview?.supportStat" class="stat-bar">
        <span>
          预计改写 <strong>{{ preview.supportStat.modifiedEntries }}</strong> {{ supportMeta.unit }}
        </span>
        <span class="stat-sample">
          示例门槛：<code>{{ fmtSeq(preview.supportStat.sampleBefore) }}</code> →
          <code>{{ fmtSeq(preview.supportStat.sampleAfter) }}</code>
        </span>
      </div>
    </section>

    <!-- 卡片 2：武器熟练度 / 战技 SP 加速 -->
    <section class="rule-card" :class="{ muted: !weaponMeta.supported }">
      <div class="card-head">
        <div>
          <h3>{{ weaponMeta.title }}</h3>
          <code class="file-tag">{{ weaponMeta.file }}</code>
        </div>
        <span class="status-pill" :class="{ active: weaponMeta.supported && currentConfig.weaponExp !== 'vanilla' }">
          {{
            !weaponMeta.supported
              ? "暂未开放"
              : currentConfig.weaponExp === "vanilla"
                ? "原版"
                : "已启用"
          }}
        </span>
      </div>
      <p class="card-note">{{ weaponMeta.note }}</p>

      <div v-if="weaponMeta.supported" class="preset-grid five-cols">
        <button v-for="opt in weaponExpOptions" :key="opt.id" class="preset-btn"
          :class="{ selected: currentConfig.weaponExp === opt.id }" :disabled="loading"
          @click="selectWeaponExp(opt.id)">
          <span class="preset-title">{{ opt.title }}</span>
          <span class="preset-desc">{{ opt.desc }}</span>
        </button>
      </div>

      <div v-if="preview?.weaponExpStat" class="stat-bar">
        <span>
          预计改写 <strong>{{ preview.weaponExpStat.modifiedEntries }}</strong> {{ weaponMeta.unit }}
        </span>
        <span class="stat-sample">
          示例数值：<code>{{ fmtSeq(preview.weaponExpStat.sampleBefore) }}</code> →
          <code>{{ fmtSeq(preview.weaponExpStat.sampleAfter) }}</code>
        </span>
      </div>
    </section>
  </div>
</template>

<style scoped>
.growth-panel {
  height: 100%;
  overflow-y: auto;
  padding: 20px 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  padding-bottom: 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
}

.panel-header h2 {
  margin: 0;
  font-size: 18px;
}

.panel-sub {
  margin: 4px 0 0;
  font-size: 12px;
  opacity: 0.72;
}

.btn-reset {
  padding: 6px 12px;
  border-radius: 6px;
  border: 1px solid rgba(255, 255, 255, 0.16);
  background: rgba(255, 255, 255, 0.06);
  color: inherit;
  cursor: pointer;
  font-size: 12px;
}

.btn-reset:disabled {
  opacity: 0.4;
  cursor: not-allowed;
}

.panel-err {
  padding: 10px 12px;
  border-radius: 8px;
  background: rgba(220, 38, 38, 0.18);
  border: 1px solid rgba(248, 113, 113, 0.45);
  font-size: 13px;
}

.rule-card {
  padding: 16px 18px;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.035);
  border: 1px solid rgba(255, 255, 255, 0.09);
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.rule-card.muted {
  opacity: 0.75;
}

.card-head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 12px;
}

.card-head h3 {
  margin: 0 0 4px;
  font-size: 15px;
}

.file-tag {
  font-size: 11px;
  opacity: 0.7;
  padding: 2px 6px;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.25);
}

.status-pill {
  font-size: 11px;
  padding: 3px 9px;
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.08);
  opacity: 0.8;
  white-space: nowrap;
}

.status-pill.active {
  background: rgba(59, 130, 246, 0.25);
  color: #93c5fd;
  border: 1px solid rgba(96, 165, 250, 0.45);
  opacity: 1;
}

.card-note {
  margin: 0;
  font-size: 12.5px;
  opacity: 0.78;
  line-height: 1.5;
}

.preset-grid {
  display: grid;
  grid-template-columns: repeat(4, minmax(0, 1fr));
  gap: 10px;
}

.preset-grid.five-cols {
  grid-template-columns: repeat(5, minmax(0, 1fr));
}

.preset-btn {
  text-align: left;
  padding: 10px 12px;
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.12);
  background: rgba(255, 255, 255, 0.03);
  color: inherit;
  cursor: pointer;
  display: flex;
  flex-direction: column;
  gap: 4px;
  transition: border-color 0.15s, background 0.15s;
}

.preset-btn:hover:not(:disabled) {
  border-color: rgba(96, 165, 250, 0.5);
  background: rgba(59, 130, 246, 0.08);
}

.preset-btn.selected {
  border-color: #60a5fa;
  background: rgba(59, 130, 246, 0.18);
}

.preset-title {
  font-size: 13px;
  font-weight: 600;
}

.preset-desc {
  font-size: 11.5px;
  opacity: 0.72;
  line-height: 1.35;
}

.stat-bar {
  display: flex;
  flex-wrap: wrap;
  justify-content: space-between;
  gap: 8px;
  padding: 9px 12px;
  border-radius: 7px;
  background: rgba(16, 185, 129, 0.12);
  border: 1px solid rgba(52, 211, 153, 0.35);
  font-size: 12.5px;
}

.stat-sample code {
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.25);
}

@media (max-width: 900px) {

  .preset-grid,
  .preset-grid.five-cols {
    grid-template-columns: repeat(2, minmax(0, 1fr));
  }
}
</style>