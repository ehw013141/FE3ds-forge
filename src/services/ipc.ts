import { invoke } from "@tauri-apps/api/core";
import type {
  AppBootstrapInfo,
  ExpPatchMode,
  ExpPatchPreview,
  ExtractReport,
  ExportReport,
  ExportWorkspaceArgs,
  TableDataPayload,
  TableRecord,
  WorkspaceSummary,
} from "../types/ipc";

function normalizeError(err: unknown): Error {
  if (err instanceof Error) return err;
  return new Error(typeof err === "string" ? err : JSON.stringify(err));
}

async function callIpc<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    throw normalizeError(err);
  }
}

export const ipc = {
  getBootstrapInfo: () => callIpc<AppBootstrapInfo>("get_bootstrap_info"),

  openWorkspace: (rootDir: string) =>
    callIpc<WorkspaceSummary>("open_workspace", { rootDir }),

  getTableData: (tableId: string) =>
    callIpc<TableDataPayload>("get_table_data", { tableId }),

  saveTableRecords: (tableId: string, records: TableRecord[]) =>
    callIpc<number>("save_table_records", { tableId, records }),

  setExpPatch: (mode: ExpPatchMode) =>
    callIpc<ExpPatchPreview>("set_exp_patch", { mode }),

  exportWorkspace: (args: ExportWorkspaceArgs) =>
    callIpc<ExportReport>("export_workspace", { args }),

  getPortrait: (recordId: string) =>
    callIpc<string | null>("get_portrait", { recordId }),

  extractRom: (outputDir: string) =>
    callIpc<ExtractReport>("extract_rom", { outputDir }),
};