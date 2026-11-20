use fe3ds_core::exefs_patcher::CODE_VADDR_BASE;
use fe3ds_core::exefs_patcher::ExpPatchMode;
use fe3ds_core::exporter::ExportReport;
use fe3ds_core::schema::TableRecord;
use fe3ds_core::vfs::ExtractReport;
use fe3ds_core::growth_rules::GrowthRulesConfig;
use tauri::State;

mod workspace;
use workspace::{
    ExpPatchPreview, ExportWorkspaceArgs, LoadedWorkspace, TableDataPayload, WorkspaceState,
    WorkspaceSummary,GrowthRulesPreview,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppBootstrapInfo {
    pub app_name: String,
    pub version: String,
    pub code_vaddr_base: u32,
    pub default_output_dir: String,
}

fn resolve_default_output_dir() -> String {
    #[cfg(debug_assertions)]
    let base_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(std::path::Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    #[cfg(not(debug_assertions))]
    let base_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    base_dir.join("output").to_string_lossy().into_owned()
}

#[tauri::command]
fn get_bootstrap_info() -> AppBootstrapInfo {
    AppBootstrapInfo {
        app_name: "FE3DS Forge".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        code_vaddr_base: CODE_VADDR_BASE,
        default_output_dir: resolve_default_output_dir(),
    }
}

#[tauri::command]
fn open_workspace(
    root_dir: String,
    state: State<'_, WorkspaceState>,
) -> Result<WorkspaceSummary, String> {
    let ws = LoadedWorkspace::load_from_path(std::path::Path::new(&root_dir))?;
    let summary = ws.summary();
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    *guard = Some(ws);
    Ok(summary)
}

#[tauri::command]
fn get_table_data(
    table_id: String,
    state: State<'_, WorkspaceState>,
) -> Result<TableDataPayload, String> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    let ws = guard.as_ref().ok_or_else(|| "尚未打开游戏目录".to_string())?;
    ws.get_table_data(&table_id)
}

#[tauri::command]
fn save_table_records(
    table_id: String,
    records: Vec<TableRecord>,
    state: State<'_, WorkspaceState>,
) -> Result<usize, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    let ws = guard.as_mut().ok_or_else(|| "尚未打开游戏目录".to_string())?;
    ws.save_table_records(&table_id, records)
}

#[tauri::command]
fn set_exp_patch(
    mode: ExpPatchMode,
    state: State<'_, WorkspaceState>,
) -> Result<ExpPatchPreview, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    let ws = guard.as_mut().ok_or_else(|| "尚未打开游戏目录".to_string())?;
    ws.set_exp_patch(mode)
}

#[tauri::command]
fn set_growth_rules(
    config: GrowthRulesConfig,
    state: State<'_, WorkspaceState>,
) -> Result<GrowthRulesPreview, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    let ws = guard.as_mut().ok_or_else(|| "尚未打开游戏目录".to_string())?;
    ws.set_growth_rules(config)
}

#[tauri::command]
fn export_workspace(
    args: ExportWorkspaceArgs,
    state: State<'_, WorkspaceState>,
) -> Result<ExportReport, String> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    let ws = guard.as_ref().ok_or_else(|| "尚未打开游戏目录".to_string())?;
    ws.export(&args)
}

#[tauri::command]
fn get_portrait(
    record_id: String,
    state: State<'_, WorkspaceState>,
) -> Result<Option<String>, String> {
    let mut guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    let ws = guard.as_mut().ok_or_else(|| "尚未打开游戏目录".to_string())?;
    ws.get_portrait_data_url(&record_id)
}

#[tauri::command]
fn extract_rom(
    output_dir: String,
    state: State<'_, WorkspaceState>,
) -> Result<ExtractReport, String> {
    let guard = state
        .inner
        .lock()
        .map_err(|_| "工作区状态锁异常".to_string())?;
    let ws = guard.as_ref().ok_or_else(|| "尚未打开游戏镜像".to_string())?;
    ws.extract_rom(&output_dir)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
tauri::Builder::default()
        .manage(WorkspaceState::default())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_bootstrap_info,
            open_workspace,
            get_table_data,
            save_table_records,
            set_exp_patch,
            set_growth_rules,
            export_workspace,
            get_portrait,
            extract_rom
        ])
        .run(tauri::generate_context!())
        .expect("启动 FE3DS Forge 桌面端失败");
}