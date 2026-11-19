export interface AppBootstrapInfo {
  appName: string;
  version: string;
  codeVaddrBase: number;
  defaultOutputDir: string;
}

export type FieldKind =
  | { type: "u8" }
  | { type: "i8" }
  | { type: "u16" }
  | { type: "i16" }
  | { type: "u32" }
  | { type: "u8_array"; len: number }
  | { type: "i8_array"; len: number }
  | { type: "ciphered_u8_array"; len: number };

export interface FieldDef {
  key: string;
  label: string;
  offset: number;
  kind: FieldKind;
  labels?: string[] | null;
  min?: number | null;
  max?: number | null;
}

export interface TableSchema {
  id: string;
  game: string;
  title: string;
  target_file: string;
  entry_prefix: string;
  entry_size: number;
  fields: FieldDef[];
}

export type FieldValue =
  | { kind: "int"; value: number }
  | { kind: "int_array"; value: number[] };

export interface TableRecord {
  index: number;
  id: string;
  display_name: string;
  offset: number;
  values: Record<string, FieldValue>;
}

export type ExpMultiplier =
  | "half"
  | "one_point_five"
  | "double"
  | "triple"
  | "quadruple"
  | "quintuple";

export type ExpPatchMode =
  | { mode: "vanilla" }
  | { mode: "multiplier"; preset: ExpMultiplier }
  | { mode: "fixed"; value: number };

export interface TableMeta {
  id: string;
  title: string;
  targetFile: string;
  recordCount: number;
  modifiedCount: number;
}

export type RomFormat = "ncsd" | "cia" | "ncch";

export interface RomMetadata {
  format: RomFormat;
  titleId: string;
  productCode: string;
  romfsFileCount: number;
  hasCodeBin: boolean;
  codeCompressed: boolean;
}

export interface ExtractReport {
  outputDir: string;
  romfsFiles: number;
  exefsFiles: number;
  totalBytes: number;
}

export interface WorkspaceSummary {
  rootDir: string;
  sourceKind: "directory" | "image";
  romMeta: RomMetadata | null;
  gameId: string;
  gameTitle: string;
  messageCount: number;
  hasCodeBin: boolean;
  expSiteOffset: number | null;
  expSiteVaddr: number | null;
  expMode: ExpPatchMode;
  tables: TableMeta[];
}

export interface TableDataPayload {
  schema: TableSchema;
  records: TableRecord[];
  originalRecords: TableRecord[];
  modifiedCount: number;
}

export interface ExpPatchPreview {
  mode: ExpPatchMode;
  active: boolean;
  fileOffset: number | null;
  vaddr: number | null;
  ipsBytes: number | null;
  cheatText: string | null;
}

export interface ExportWorkspaceArgs {
  outputDir: string;
  exportIps: boolean;
  exportCheat: boolean;
  cheatTitle: string;
}

export interface ExportReport {
  written_files: string[];
  romfs_count: number;
  ips_bytes: number | null;
  cheat_written: boolean;
}