export interface ShortcutModifiers {
  command: boolean;
  option: boolean;
  control: boolean;
  shift: boolean;
}

export type ShortcutKind =
  | { type: "right_option" }
  | { type: "left_option" }
  | { type: "right_command" }
  | { type: "right_control" }
  | { type: "fn_key" }
  | {
      type: "key_combo";
      key_code: number;
      modifiers: ShortcutModifiers;
      key_name: string;
    };

export interface ShortcutConfig {
  kind: ShortcutKind;
  hold_ms: number;
}

export type ModelVariant = "fp16" | "q8_0";

export type ModelDeleteTarget = "fp16" | "q8_0" | "all";

export interface ModelStorageInfo {
  models_dir: string;
  fp16_installed: boolean;
  fp16_bytes: number;
  q8_0_installed: boolean;
  q8_0_bytes: number;
  total_bytes: number;
}

export interface AppConfig {
  shortcut: ShortcutConfig;
  launch_at_login: boolean;
  keywords_raw: string;
  first_notice_accepted: boolean;
  model_variant: ModelVariant;
}

export interface KeywordUsage {
  tokens: number;
  limit: number;
  remaining_tokens: number;
  keyword_count: number;
  usable_keyword_count: number;
  usable_keywords: string[];
  is_over_limit: boolean;
}

export type PermissionState = "granted" | "denied" | "not_determined";

export interface PermissionSnapshot {
  microphone: PermissionState;
  accessibility: PermissionState;
}

export type ModelReadiness =
  | { state: "not_installed" }
  | {
      state: "downloading";
      progress: number;
      downloaded_bytes: number;
      total_bytes: number;
      stage: string;
    }
  | { state: "loading" }
  | { state: "ready"; model_name: string }
  | { state: "error"; message: string };

export type AppUpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "up_to_date" }
  | {
      status: "available";
      latest_version: string;
      download_url: string;
      release_notes?: string;
    }
  | { status: "applying"; latest_version: string }
  | { status: "error"; message: string };

export type SessionPhase =
  | { phase: "idle" }
  | { phase: "first_run_notice" }
  | { phase: "blocked_secure_input"; message: string }
  | {
      phase: "recording";
      recording_id: string;
      elapsed_secs: number;
      rms_level: number;
    }
  | { phase: "transcribing"; recording_id: string }
  | {
      phase: "recovery_card";
      transcript: string;
      reason: string;
      remaining_fraction: number;
    }
  | { phase: "failed"; message: string };

export interface AppSnapshot {
  version: string;
  config: AppConfig;
  shortcut_label: string;
  keyword_usage: KeywordUsage;
  permissions: PermissionSnapshot;
  model_readiness: ModelReadiness;
  model_storage: ModelStorageInfo;
  app_update: AppUpdateState;
  session_phase: SessionPhase;
}
