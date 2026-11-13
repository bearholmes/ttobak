import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AppSnapshot,
  KeywordUsage,
  ModelDeleteTarget,
  ModelVariant,
  ShortcutConfig,
} from "./types";

const FALLBACK_SNAPSHOT: AppSnapshot = {
  version: "0.1.0",
  config: {
    shortcut: { kind: { type: "right_option" }, hold_ms: 0 },
    launch_at_login: false,
    keywords_raw: "",
    first_notice_accepted: true,
    model_variant: "fp16",
  },
  shortcut_label: "우측 ⌥",
  keyword_usage: {
    tokens: 0,
    limit: 80,
    remaining_tokens: 80,
    keyword_count: 0,
    usable_keyword_count: 0,
    usable_keywords: [],
    is_over_limit: false,
  },
  permissions: {
    microphone: "granted",
    accessibility: "granted",
  },
  model_readiness: {
    state: "ready",
    model_name: "whisper-large-v3-turbo (FP16 · Metal)",
  },
  model_storage: {
    models_dir: "~/Library/Application Support/com.ttobak.app/SpeechModels",
    fp16_installed: true,
    fp16_bytes: 1624555275,
    q8_0_installed: false,
    q8_0_bytes: 0,
    total_bytes: 1628200000,
  },
  app_update: {
    status: "idle",
  },
  session_phase: { phase: "idle" },
};

function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export function useTtobakSnapshot() {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);

  const refresh = useCallback(async () => {
    if (!isTauriRuntime()) {
      setSnapshot(FALLBACK_SNAPSHOT);
      return;
    }
    try {
      const snap = await invoke<AppSnapshot>("get_snapshot");
      setSnapshot(snap);
    } catch (err) {
      console.error("스냅샷 조회 실패:", err);
    }
  }, []);

  useEffect(() => {
    void refresh();
    if (!isTauriRuntime()) return;

    let unlistenFn: (() => void) | undefined;
    listen<AppSnapshot>("ttobak://snapshot", (event) => {
      setSnapshot(event.payload);
    }).then((fn) => {
      unlistenFn = fn;
    });

    return () => {
      if (unlistenFn) unlistenFn();
    };
  }, [refresh]);

  return { snapshot: snapshot ?? FALLBACK_SNAPSHOT, refresh };
}

export const ttobakApi = {
  toggleRecording: () =>
    isTauriRuntime() ? invoke<void>("toggle_recording") : Promise.resolve(),
  cancelSession: () =>
    isTauriRuntime() ? invoke<void>("cancel_session") : Promise.resolve(),
  acceptFirstNotice: () =>
    isTauriRuntime() ? invoke<void>("accept_first_notice") : Promise.resolve(),
  updateShortcut: (shortcut: ShortcutConfig) =>
    isTauriRuntime()
      ? invoke<void>("update_shortcut", { shortcut })
      : Promise.resolve(),
  setShortcutRecordingPaused: (paused: boolean) =>
    isTauriRuntime()
      ? invoke<void>("set_shortcut_recording_paused", { paused })
      : Promise.resolve(),
  updateKeywords: (keywordsRaw: string) =>
    isTauriRuntime()
      ? invoke<KeywordUsage>("update_keywords", { keywordsRaw })
      : Promise.resolve(FALLBACK_SNAPSHOT.keyword_usage),
  previewKeywordUsage: (keywordsRaw: string) =>
    isTauriRuntime()
      ? invoke<KeywordUsage>("preview_keyword_usage", { keywordsRaw })
      : Promise.resolve(FALLBACK_SNAPSHOT.keyword_usage),
  setLaunchAtLogin: (enabled: boolean) =>
    isTauriRuntime()
      ? invoke<void>("set_launch_at_login", { enabled })
      : Promise.resolve(),
  updateModelVariant: (variant: ModelVariant) =>
    isTauriRuntime()
      ? invoke<void>("update_model_variant", { variant })
      : Promise.resolve(),
  downloadModels: () =>
    isTauriRuntime() ? invoke<void>("download_models") : Promise.resolve(),
  deleteModels: (target: ModelDeleteTarget) =>
    isTauriRuntime()
      ? invoke<void>("delete_models", { target })
      : Promise.resolve(),
  revealModelsFolder: () =>
    isTauriRuntime()
      ? invoke<void>("reveal_models_folder")
      : Promise.resolve(),
  openExternalUrl: (url: string) =>
    isTauriRuntime()
      ? invoke<void>("open_external_url", { url })
      : Promise.resolve(window.open(url, "_blank", "noopener,noreferrer") as unknown as void),
  checkAppUpdate: () =>
    isTauriRuntime() ? invoke<void>("check_app_update") : Promise.resolve(),
  applyAppUpdate: () =>
    isTauriRuntime() ? invoke<void>("apply_app_update") : Promise.resolve(),
  requestOrOpenPermission: (kind: "microphone" | "accessibility") =>
    isTauriRuntime()
      ? invoke<void>("request_or_open_permission", { kind })
      : Promise.resolve(),
  refreshPermissions: () =>
    isTauriRuntime()
      ? invoke<AppSnapshot>("refresh_permissions")
      : Promise.resolve(FALLBACK_SNAPSHOT),
  copyRecoveryTranscript: () =>
    isTauriRuntime()
      ? invoke<void>("copy_recovery_transcript")
      : Promise.resolve(),
  dismissRecovery: () =>
    isTauriRuntime() ? invoke<void>("dismiss_recovery") : Promise.resolve(),
  openSettings: () =>
    isTauriRuntime() ? invoke<void>("open_settings") : Promise.resolve(),
  hideMenuPopover: () =>
    isTauriRuntime() ? invoke<void>("hide_menu_popover") : Promise.resolve(),
  quitApp: () =>
    isTauriRuntime() ? invoke<void>("quit_app") : Promise.resolve(),
};
