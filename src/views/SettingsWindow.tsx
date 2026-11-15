import React, { useEffect, useRef, useState } from "react";
import {
  SlidersHorizontal,
  BookOpen,
  Lock,
  Download,
  Loader2,
  RefreshCw,
  ShieldCheck,
  Trash2,
  FolderOpen,
  Scale,
  ExternalLink,
  Plus,
  X,
  ChevronDown,
} from "lucide-react";
import type {
  AppSnapshot,
  KeywordUsage,
  ModelDeleteTarget,
  ModelVariant,
  ShortcutConfig,
  ShortcutKind,
} from "../types";
import { ttobakApi } from "../api";
import { TtobakEmblem } from "../components/TtobakEmblem";
import { OPEN_SOURCE_CATEGORIES } from "../data/openSourceLicenses";

interface Props {
  snapshot: AppSnapshot;
}

type TabKey = "general" | "keywords" | "privacy" | "licenses";

interface ModelVariantOption {
  value: ModelVariant;
  group: string;
  optionLabel: string;
  shortLabel: string;
  sizeLabel: string;
  desc: string;
}

const MODEL_VARIANT_OPTIONS: ModelVariantOption[] = [
  {
    value: "fp16",
    group: "Whisper large-v3-turbo (범용)",
    optionLabel: "Whisper Turbo · 표준 (1.62GB)",
    shortLabel: "Whisper Turbo 표준",
    sizeLabel: "1.62GB",
    desc: "FP16 무손실 · 한국어/영어 범용",
  },
  {
    value: "q8_0",
    group: "Whisper large-v3-turbo (범용)",
    optionLabel: "Whisper Turbo · 경량 Q8 (874MB)",
    shortLabel: "Whisper Turbo 경량",
    sizeLabel: "874MB",
    desc: "Q8_0 양자화 · 메모리 절약",
  },
  {
    value: "batisay_ko_base",
    group: "batisay-ko-base (한국어 특화)",
    optionLabel: "batisay-ko-base · 표준 (1.62GB)",
    shortLabel: "batisay-ko-base 표준",
    sizeLabel: "1.62GB",
    desc: "한국어 파인튜닝 · FP16 고품질",
  },
  {
    value: "batisay_ko_base_q5_0",
    group: "batisay-ko-base (한국어 특화)",
    optionLabel: "batisay-ko-base · 경량 Q5 (574MB)",
    shortLabel: "batisay-ko-base 경량",
    sizeLabel: "574MB",
    desc: "한국어 파인튜닝 · Q5_0 초경량",
  },
];

const MODEL_VARIANT_GROUPS = Array.from(
  new Set(MODEL_VARIANT_OPTIONS.map((opt) => opt.group)),
);

function getModelVariantOption(variant: ModelVariant): ModelVariantOption {
  return (
    MODEL_VARIANT_OPTIONS.find((opt) => opt.value === variant) ??
    MODEL_VARIANT_OPTIONS[0]
  );
}

function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 MB";
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) {
    return `${(mb / 1024).toFixed(2)} GB`;
  }
  return `${mb.toFixed(0)} MB`;
}

function parseKeywordList(raw: string): string[] {
  const seen: string[] = [];
  for (const part of raw.split(/[,\n]/)) {
    const trimmed = part.trim();
    if (trimmed && !seen.includes(trimmed)) {
      seen.push(trimmed);
    }
  }
  return seen;
}

const DOM_CODE_TO_MAC_KEYCODE: Record<string, { code: number; label: string }> =
  {
    Space: { code: 49, label: "Space" },
    Enter: { code: 36, label: "Return" },
    Tab: { code: 48, label: "Tab" },
    Backquote: { code: 50, label: "`" },
    Minus: { code: 27, label: "-" },
    Equal: { code: 24, label: "=" },
    BracketLeft: { code: 33, label: "[" },
    BracketRight: { code: 30, label: "]" },
    Backslash: { code: 42, label: "\\" },
    Period: { code: 47, label: "." },
    Comma: { code: 43, label: "," },
    Slash: { code: 44, label: "/" },
    Semicolon: { code: 41, label: ";" },
    Quote: { code: 39, label: "'" },
    KeyA: { code: 0, label: "A" },
    KeyS: { code: 1, label: "S" },
    KeyD: { code: 2, label: "D" },
    KeyF: { code: 3, label: "F" },
    KeyH: { code: 4, label: "H" },
    KeyG: { code: 5, label: "G" },
    KeyZ: { code: 6, label: "Z" },
    KeyX: { code: 7, label: "X" },
    KeyC: { code: 8, label: "C" },
    KeyV: { code: 9, label: "V" },
    KeyB: { code: 11, label: "B" },
    KeyQ: { code: 12, label: "Q" },
    KeyW: { code: 13, label: "W" },
    KeyE: { code: 14, label: "E" },
    KeyR: { code: 15, label: "R" },
    KeyY: { code: 16, label: "Y" },
    KeyT: { code: 17, label: "T" },
    Digit1: { code: 18, label: "1" },
    Digit2: { code: 19, label: "2" },
    Digit3: { code: 20, label: "3" },
    Digit4: { code: 21, label: "4" },
    Digit6: { code: 22, label: "6" },
    Digit5: { code: 23, label: "5" },
    Digit9: { code: 25, label: "9" },
    Digit7: { code: 26, label: "7" },
    Digit8: { code: 28, label: "8" },
    Digit0: { code: 29, label: "0" },
    KeyO: { code: 31, label: "O" },
    KeyU: { code: 32, label: "U" },
    KeyI: { code: 34, label: "I" },
    KeyP: { code: 35, label: "P" },
    KeyL: { code: 37, label: "L" },
    KeyJ: { code: 38, label: "J" },
    KeyK: { code: 40, label: "K" },
    KeyN: { code: 45, label: "N" },
    KeyM: { code: 46, label: "M" },
    F1: { code: 122, label: "F1" },
    F2: { code: 120, label: "F2" },
    F3: { code: 99, label: "F3" },
    F4: { code: 118, label: "F4" },
    F5: { code: 96, label: "F5" },
    F6: { code: 97, label: "F6" },
    F7: { code: 98, label: "F7" },
    F8: { code: 100, label: "F8" },
    F9: { code: 101, label: "F9" },
    F10: { code: 109, label: "F10" },
    F11: { code: 103, label: "F11" },
    F12: { code: 111, label: "F12" },
  };

function renderKindLabel(kind: ShortcutKind): string {
  switch (kind.type) {
    case "right_option":
      return "우측 ⌥";
    case "left_option":
      return "좌측 ⌥";
    case "right_command":
      return "우측 ⌘";
    case "right_control":
      return "우측 ⌃";
    case "fn_key":
      return "Fn";
    case "key_combo": {
      const parts: string[] = [];
      if (kind.modifiers.control) parts.push("⌃");
      if (kind.modifiers.option) parts.push("⌥");
      if (kind.modifiers.shift) parts.push("⇧");
      if (kind.modifiers.command) parts.push("⌘");
      parts.push(kind.key_name);
      return parts.join(" ");
    }
  }
}

export const SettingsWindow: React.FC<Props> = ({ snapshot }) => {
  const [activeTab, setActiveTab] = useState<TabKey>("general");
  const [keywordsList, setKeywordsList] = useState<string[]>(() =>
    parseKeywordList(snapshot.config.keywords_raw),
  );
  const [keywordInput, setKeywordInput] = useState("");
  const [liveUsage, setLiveUsage] = useState<KeywordUsage>(
    snapshot.keyword_usage,
  );
  const [savedToast, setSavedToast] = useState(false);
  const toastTimerRef = useRef<number | null>(null);

  const [shortcutModalOpen, setShortcutModalOpen] = useState(false);
  const [draftShortcut, setDraftShortcut] = useState<ShortcutConfig>(
    snapshot.config.shortcut,
  );

  const [deleteModalOpen, setDeleteModalOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<ModelDeleteTarget>("all");
  const [deletingModel, setDeletingModel] = useState(false);

  useEffect(() => {
    setKeywordsList(parseKeywordList(snapshot.config.keywords_raw));
    setLiveUsage(snapshot.keyword_usage);
  }, [snapshot.config.keywords_raw, snapshot.keyword_usage]);

  useEffect(() => {
    return () => {
      if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
    };
  }, []);

  const flashSaved = () => {
    setSavedToast(true);
    if (toastTimerRef.current) window.clearTimeout(toastTimerRef.current);
    toastTimerRef.current = window.setTimeout(() => setSavedToast(false), 1400);
  };

  useEffect(() => {
    if (!shortcutModalOpen) return;
    void ttobakApi.setShortcutRecordingPaused(true);

    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (
        e.code === "Escape" &&
        !e.metaKey &&
        !e.altKey &&
        !e.ctrlKey &&
        !e.shiftKey
      ) {
        setShortcutModalOpen(false);
        return;
      }

      if (e.code === "AltRight") {
        setDraftShortcut((prev) => ({
          ...prev,
          kind: { type: "right_option" },
        }));
        return;
      }
      if (e.code === "AltLeft") {
        setDraftShortcut((prev) => ({
          ...prev,
          kind: { type: "left_option" },
        }));
        return;
      }
      if (e.code === "MetaRight") {
        setDraftShortcut((prev) => ({
          ...prev,
          kind: { type: "right_command" },
        }));
        return;
      }
      if (e.code === "ControlRight") {
        setDraftShortcut((prev) => ({
          ...prev,
          kind: { type: "right_control" },
        }));
        return;
      }

      const mapped = DOM_CODE_TO_MAC_KEYCODE[e.code];
      if (mapped && (e.metaKey || e.altKey || e.ctrlKey || e.shiftKey)) {
        setDraftShortcut((prev) => ({
          ...prev,
          kind: {
            type: "key_combo",
            key_code: mapped.code,
            key_name: mapped.label,
            modifiers: {
              command: e.metaKey,
              option: e.altKey,
              control: e.ctrlKey,
              shift: e.shiftKey,
            },
          },
        }));
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      void ttobakApi.setShortcutRecordingPaused(false);
    };
  }, [shortcutModalOpen]);

  const handleKeywordInputChange = async (val: string) => {
    setKeywordInput(val);
    const pending = parseKeywordList(val);
    const previewMerged = [...keywordsList];
    for (const w of pending) {
      if (!previewMerged.includes(w)) previewMerged.push(w);
    }
    const preview = await ttobakApi.previewKeywordUsage(
      previewMerged.join(", "),
    );
    setLiveUsage(preview);
  };

  const handleAddKeywords = async () => {
    const incoming = parseKeywordList(keywordInput);
    if (incoming.length === 0) return;

    const merged = [...keywordsList];
    for (const w of incoming) {
      if (!merged.includes(w)) merged.push(w);
    }
    setKeywordsList(merged);
    setKeywordInput("");
    const updated = await ttobakApi.updateKeywords(merged.join(", "));
    setLiveUsage(updated);
    flashSaved();
  };

  const handleRemoveKeyword = async (word: string) => {
    const next = keywordsList.filter((w) => w !== word);
    setKeywordsList(next);
    const updated = await ttobakApi.updateKeywords(next.join(", "));
    setLiveUsage(updated);
    flashSaved();
  };

  const handleClearAllKeywords = async () => {
    setKeywordsList([]);
    setKeywordInput("");
    const updated = await ttobakApi.updateKeywords("");
    setLiveUsage(updated);
    flashSaved();
  };

  const handleSaveShortcut = async () => {
    await ttobakApi.updateShortcut(draftShortcut);
    setShortcutModalOpen(false);
  };

  const showFirstRunModal =
    !snapshot.config.first_notice_accepted ||
    snapshot.session_phase.phase === "first_run_notice";

  const readiness = snapshot.model_readiness;
  const storage = snapshot.model_storage;
  const appUpdate = snapshot.app_update;
  const currentVariant: ModelVariant = snapshot.config.model_variant ?? "fp16";
  const currentVariantMeta = getModelVariantOption(currentVariant);
  const variantSizeLabel = currentVariantMeta.sizeLabel;
  const variantsOnDisk = (storage.variants ?? []).filter((v) => v.bytes > 0);
  const hasMultipleVariantsOnDisk = variantsOnDisk.length > 1;
  const hasAnyModelFiles = storage.total_bytes > 0;
  const isModelBusy =
    readiness.state === "downloading" ||
    readiness.state === "loading" ||
    snapshot.session_phase.phase === "recording" ||
    snapshot.session_phase.phase === "transcribing" ||
    deletingModel;
  const isUpdateBusy =
    appUpdate.status === "checking" || appUpdate.status === "applying";

  const openDeleteModelModal = () => {
    setDeleteTarget("all");
    setDeleteModalOpen(true);
  };

  const handleConfirmDeleteModel = async () => {
    setDeletingModel(true);
    try {
      await ttobakApi.deleteModels(deleteTarget);
      setDeleteModalOpen(false);
    } finally {
      setDeletingModel(false);
    }
  };

  const renderUpdateStatusText = (): string | null => {
    switch (appUpdate.status) {
      case "idle":
        return null;
      case "checking":
        return "업데이트 확인 중...";
      case "up_to_date":
        return "최신 버전입니다.";
      case "available":
        return "새 버전 업데이트 가능";
      case "applying":
        return `v${appUpdate.latest_version} 설치 파일 다운로드 및 여는 중...`;
      case "error":
        return appUpdate.message;
    }
  };

  const handleVariantSelect = async (variant: ModelVariant) => {
    if (isModelBusy || variant === currentVariant) return;
    await ttobakApi.updateModelVariant(variant);
  };

  const micGranted = snapshot.permissions.microphone === "granted";
  const accGranted = snapshot.permissions.accessibility === "granted";
  const allReady = readiness.state === "ready" && micGranted && accGranted;
  const updateDesc = renderUpdateStatusText();
  const usableSet = new Set(liveUsage.usable_keywords);

  const renderSidebarStatus = () => {
    if (allReady) {
      return (
        <span className="sidebar-status">
          <span className="status-dot ok" />
          사용 가능
        </span>
      );
    }
    if (readiness.state === "downloading") {
      return (
        <span className="sidebar-status busy">
          <Loader2 size={10.5} className="spin" />
          다운로드 {Math.round(readiness.progress * 100)}%
        </span>
      );
    }
    if (readiness.state === "loading") {
      return (
        <span className="sidebar-status busy">
          <Loader2 size={10.5} className="spin" />
          엔진 준비 중
        </span>
      );
    }
    return (
      <span className="sidebar-status warn">
        <span className="status-dot warn" />
        설정 필요
      </span>
    );
  };

  return (
    <div className="settings-root">
      {/* 좌측 사이드바 */}
      <aside className="settings-sidebar" aria-label="설정 탐색">
        <div className="sidebar-top">
          <div className="sidebar-brand">
            <TtobakEmblem
              size={28}
              active={snapshot.session_phase.phase === "recording"}
            />
            <div className="sidebar-brand-meta">
              <div className="sidebar-brand-row">
                <span className="sidebar-app-name">또박</span>
                <span className="sidebar-version">v{snapshot.version}</span>
              </div>
              {renderSidebarStatus()}
            </div>
          </div>

          <nav className="sidebar-nav">
            <button
              type="button"
              className={`sidebar-nav-btn ${
                activeTab === "general" ? "active" : ""
              }`}
              onClick={() => setActiveTab("general")}
            >
              <SlidersHorizontal size={14} strokeWidth={2} />
              <span>일반</span>
            </button>
            <button
              type="button"
              className={`sidebar-nav-btn ${
                activeTab === "keywords" ? "active" : ""
              }`}
              onClick={() => setActiveTab("keywords")}
            >
              <BookOpen size={14} strokeWidth={2} />
              <span>용어 사전</span>
            </button>
            <button
              type="button"
              className={`sidebar-nav-btn ${
                activeTab === "privacy" ? "active" : ""
              }`}
              onClick={() => setActiveTab("privacy")}
            >
              <Lock size={14} strokeWidth={2} />
              <span>개인정보</span>
            </button>
            <button
              type="button"
              className={`sidebar-nav-btn ${
                activeTab === "licenses" ? "active" : ""
              }`}
              onClick={() => setActiveTab("licenses")}
            >
              <Scale size={14} strokeWidth={2} />
              <span>라이선스</span>
            </button>
          </nav>
        </div>
      </aside>

      {/* 우측 본문 */}
      <main className="settings-body">
        {activeTab === "general" && (
          <div className="tab-pane">
            <div className="pane-topbar">
              <h1 className="pane-heading">일반</h1>
            </div>

            <div className="section-block">
              <div className="section-label">입력 및 실행</div>
              <div className="setting-group">
                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">말하기 단축키</div>
                      <div className="row-desc">
                        한 번 눌러 시작·완료 · 취소 <kbd>Esc</kbd>
                      </div>
                    </div>
                  </div>
                  <div className="setting-right">
                    <div className="shortcut-pill-display">
                      {snapshot.shortcut_label}
                      {snapshot.config.shortcut.hold_ms > 0 && (
                        <small>
                          {" "}
                          ·{" "}
                          {(snapshot.config.shortcut.hold_ms / 1000).toFixed(1)}
                          초
                        </small>
                      )}
                    </div>
                    <button
                      type="button"
                      className="soft-btn"
                      onClick={() => {
                        setDraftShortcut(snapshot.config.shortcut);
                        setShortcutModalOpen(true);
                      }}
                    >
                      변경
                    </button>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left">
                    <div className="row-title">로그인 시 자동 시작</div>
                  </div>
                  <div className="setting-right">
                    <button
                      type="button"
                      role="switch"
                      aria-checked={snapshot.config.launch_at_login}
                      className={`mac-switch ${
                        snapshot.config.launch_at_login ? "on" : ""
                      }`}
                      onClick={() =>
                        void ttobakApi.setLaunchAtLogin(
                          !snapshot.config.launch_at_login,
                        )
                      }
                    >
                      <span className="mac-switch-knob" />
                    </button>
                  </div>
                </div>
              </div>
            </div>

            <div className="section-block">
              <div className="section-head-flex">
                <span className="section-label">권한</span>
                <button
                  type="button"
                  className="ghost-refresh-btn"
                  onClick={() => void ttobakApi.refreshPermissions()}
                >
                  <RefreshCw size={11} /> 새로고침
                </button>
              </div>

              <div className="setting-group">
                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">마이크 접근</div>
                      {!micGranted && (
                        <div className="row-desc">
                          음성 입력을 위해 필요합니다.
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="setting-right">
                    <span
                      className={`inline-state-badge ${
                        micGranted ? "ok" : "warn"
                      }`}
                    >
                      <span
                        className={`status-dot ${micGranted ? "ok" : "warn"}`}
                      />
                      {micGranted ? "허용됨" : "필요"}
                    </span>
                    <button
                      type="button"
                      className={micGranted ? "soft-btn" : "primary-btn"}
                      onClick={() =>
                        void ttobakApi.requestOrOpenPermission("microphone")
                      }
                    >
                      {micGranted ? "설정" : "허용하기"}
                    </button>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">손쉬운 사용</div>
                      <div className="row-desc">커서 위치 텍스트 자동 입력</div>
                      {!accGranted && (
                        <div className="row-sub-hint">
                          켜져 있는데도 미허용이면 시스템 설정 목록에서 ‘또박’을{" "}
                          <kbd>−</kbd>로 지운 뒤 다시 추가해 주세요.
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="setting-right">
                    <span
                      className={`inline-state-badge ${
                        accGranted ? "ok" : "warn"
                      }`}
                    >
                      <span
                        className={`status-dot ${accGranted ? "ok" : "warn"}`}
                      />
                      {accGranted ? "허용됨" : "필요"}
                    </span>
                    <button
                      type="button"
                      className={accGranted ? "soft-btn" : "primary-btn"}
                      onClick={() =>
                        void ttobakApi.requestOrOpenPermission("accessibility")
                      }
                    >
                      {accGranted ? "설정" : "허용하기"}
                    </button>
                  </div>
                </div>
              </div>
            </div>

            <div className="section-block">
              <div className="section-label">음성 엔진</div>
              <div className="setting-group">
                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">음성 모델</div>
                      <div className="row-desc">{currentVariantMeta.desc}</div>
                    </div>
                  </div>
                  <div className="setting-right">
                    <div className="model-select-wrap">
                      <select
                        className="model-select"
                        aria-label="음성 모델 선택"
                        disabled={isModelBusy}
                        value={currentVariant}
                        onChange={(e) =>
                          void handleVariantSelect(
                            e.target.value as ModelVariant,
                          )
                        }
                      >
                        {MODEL_VARIANT_GROUPS.map((group) => (
                          <optgroup key={group} label={group}>
                            {MODEL_VARIANT_OPTIONS.filter(
                              (opt) => opt.group === group,
                            ).map((opt) => (
                              <option key={opt.value} value={opt.value}>
                                {opt.optionLabel}
                              </option>
                            ))}
                          </optgroup>
                        ))}
                      </select>
                      <ChevronDown
                        size={13}
                        className="model-select-chevron"
                        aria-hidden="true"
                      />
                    </div>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left grow">
                    <div className="setting-left-body">
                      <div className="row-title">온디바이스 모델</div>
                      <div className="row-desc">
                        {readiness.state === "ready"
                          ? `${readiness.model_name} · ${formatBytes(
                              storage.total_bytes,
                            )}${
                              hasMultipleVariantsOnDisk
                                ? ` (${variantsOnDisk.length}개 모델 보관됨)`
                                : ""
                            }`
                          : readiness.state === "downloading"
                            ? `${readiness.stage} · ${formatBytes(
                                readiness.downloaded_bytes,
                              )} / ${formatBytes(
                                readiness.total_bytes,
                              )} (${Math.round(readiness.progress * 100)}%)`
                            : readiness.state === "loading"
                              ? "Metal 엔진 불러오는 중..."
                              : `미설치${
                                  hasAnyModelFiles
                                    ? ` · 기타 ${formatBytes(
                                        storage.total_bytes,
                                      )} 보관 중`
                                    : ""
                                }`}
                      </div>
                      {readiness.state === "downloading" && (
                        <div className="progress-track inline-model-progress">
                          <div
                            className="progress-fill"
                            style={{
                              width: `${Math.max(
                                2,
                                Math.round(readiness.progress * 100),
                              )}%`,
                            }}
                          />
                        </div>
                      )}
                      {readiness.state === "error" && (
                        <div className="row-desc error-text">
                          {readiness.message}
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="setting-right">
                    {readiness.state === "ready" ? (
                      <>
                        <span className="inline-state-badge ok">
                          <span className="status-dot ok" /> 준비됨
                        </span>
                        <button
                          type="button"
                          className="soft-danger-btn"
                          disabled={isModelBusy}
                          onClick={openDeleteModelModal}
                        >
                          <Trash2 size={12} />
                          <span>삭제</span>
                        </button>
                      </>
                    ) : readiness.state === "downloading" ||
                      readiness.state === "loading" ? (
                      <span className="inline-state-badge busy">
                        <Loader2 size={12} className="spin" />{" "}
                        {readiness.state === "downloading"
                          ? `${Math.round(readiness.progress * 100)}%`
                          : "준비 중"}
                      </span>
                    ) : (
                      <>
                        {hasAnyModelFiles && (
                          <button
                            type="button"
                            className="soft-danger-btn"
                            disabled={isModelBusy}
                            onClick={openDeleteModelModal}
                          >
                            <Trash2 size={12} />
                            <span>삭제</span>
                          </button>
                        )}
                        <button
                          type="button"
                          className="primary-btn"
                          onClick={() => void ttobakApi.downloadModels()}
                        >
                          <Download size={12.5} />
                          <span>다운로드 ({variantSizeLabel})</span>
                        </button>
                      </>
                    )}
                  </div>
                </div>
              </div>
            </div>

            <div className="section-block">
              <div className="section-label">업데이트</div>
              <div className="setting-group">
                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title-inline">
                        <span className="row-title">
                          버전 v{snapshot.version}
                        </span>
                        {appUpdate.status === "available" && (
                          <span className="version-badge-new">
                            NEW v{appUpdate.latest_version}
                          </span>
                        )}
                      </div>
                      {updateDesc && (
                        <div
                          className={`row-desc ${
                            appUpdate.status === "error" ? "error-text" : ""
                          }`}
                        >
                          {updateDesc}
                        </div>
                      )}
                    </div>
                  </div>
                  <div className="setting-right">
                    {appUpdate.status === "available" ? (
                      <>
                        <button
                          type="button"
                          className="soft-btn"
                          disabled={isUpdateBusy}
                          onClick={() => void ttobakApi.checkAppUpdate()}
                        >
                          다시 확인
                        </button>
                        <button
                          type="button"
                          className="primary-btn"
                          onClick={() => void ttobakApi.applyAppUpdate()}
                        >
                          업데이트 적용
                        </button>
                      </>
                    ) : (
                      <button
                        type="button"
                        className="soft-btn"
                        disabled={isUpdateBusy}
                        onClick={() => void ttobakApi.checkAppUpdate()}
                      >
                        {appUpdate.status === "checking"
                          ? "확인 중..."
                          : appUpdate.status === "applying"
                            ? "준비 중..."
                            : "업데이트 확인"}
                      </button>
                    )}
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {activeTab === "keywords" && (
          <div className="tab-pane">
            <div className="pane-topbar">
              <h1 className="pane-heading">용어 사전</h1>
              <div className="dict-status-right">
                {savedToast && (
                  <span className="dict-saved-toast">저장됨 ✓</span>
                )}
                <div
                  className={`budget-pill ${
                    liveUsage.is_over_limit ? "over" : ""
                  }`}
                >
                  {liveUsage.is_over_limit
                    ? `${liveUsage.limit}토큰 초과 (${liveUsage.usable_keyword_count}/${keywordsList.length}개 적용)`
                    : `${liveUsage.tokens} / ${liveUsage.limit} 토큰`}
                </div>
              </div>
            </div>

            {/* 단어 입력 및 등록 목록 */}
            <div className="keywords-box">
              <div className="dict-input-row">
                <input
                  type="text"
                  className="dict-input"
                  placeholder="등록할 단어 입력 후 Enter (쉼표로 여러 개 추가 가능)"
                  value={keywordInput}
                  onChange={(e) =>
                    void handleKeywordInputChange(e.target.value)
                  }
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && !e.nativeEvent.isComposing) {
                      e.preventDefault();
                      void handleAddKeywords();
                    }
                  }}
                />
                <button
                  type="button"
                  className="primary-btn"
                  disabled={!keywordInput.trim()}
                  onClick={() => void handleAddKeywords()}
                >
                  <Plus size={13} />
                  <span>추가</span>
                </button>
              </div>

              <div className="progress-track">
                <div
                  className={`progress-fill ${
                    liveUsage.is_over_limit ? "over" : ""
                  }`}
                  style={{
                    width: `${Math.min(
                      100,
                      Math.round((liveUsage.tokens / liveUsage.limit) * 100),
                    )}%`,
                  }}
                />
              </div>

              <div className="dict-list-header">
                <span className="dict-count-label">
                  등록된 용어 {keywordsList.length}개
                </span>
                {keywordsList.length > 0 && (
                  <button
                    type="button"
                    className="ghost-refresh-btn"
                    onClick={() => void handleClearAllKeywords()}
                  >
                    전체 삭제
                  </button>
                )}
              </div>

              <div className="dict-chip-area">
                {keywordsList.length > 0 ? (
                  keywordsList.map((kw) => {
                    const isActive =
                      !liveUsage.is_over_limit || usableSet.has(kw);
                    return (
                      <span
                        key={kw}
                        className={`kw-chip ${isActive ? "" : "inactive"}`}
                        title={
                          isActive
                            ? "음성 인식 시 반영됨"
                            : `${liveUsage.limit}토큰 한도 초과로 반영되지 않음`
                        }
                      >
                        <span>{kw}</span>
                        <button
                          type="button"
                          className="kw-chip-del"
                          onClick={() => void handleRemoveKeyword(kw)}
                          aria-label={`${kw} 삭제`}
                        >
                          <X size={11} />
                        </button>
                      </span>
                    );
                  })
                ) : (
                  <span className="kw-empty-hint">
                    등록된 용어가 없습니다. 예: 또박, Tauri, 품의서, Claude
                  </span>
                )}
              </div>
            </div>

            {/* 용어 사전 상세 안내 (항목별 명확한 정리) */}
            <div className="section-block">
              <div className="section-label">사전 동작 및 등록 기준</div>
              <div className="setting-group">
                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">어떤 단어를 넣나요?</div>
                      <div className="row-desc">
                        발음이 비슷해 자주 오인식되는 인명,
                        회사·제품·프로젝트명, 사내 약어, 영문 기술 용어의 정확한
                        표기를 등록합니다.
                      </div>
                    </div>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">어떻게 인식률을 높이나요?</div>
                      <div className="row-desc">
                        등록된 용어를 Whisper 음성 모델의 초기 컨텍스트
                        프롬프트로 전달해 해당 표기를 우선 선택하도록
                        유도합니다.
                      </div>
                    </div>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">
                        {liveUsage.limit}토큰 용량 제한
                      </div>
                      <div className="row-desc">
                        환각 현상과 문장 스타일 깨짐을 방지하기 위해 최대{" "}
                        {liveUsage.limit}토큰(한글 약 15~25단어, 영문 약
                        30~45단어)까지만 반영됩니다. 이미 잘 인식되는 일반
                        단어는 비워 두세요.
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {activeTab === "privacy" && (
          <div className="tab-pane">
            <div className="pane-topbar">
              <h1 className="pane-heading">개인정보</h1>
            </div>

            <div className="section-block">
              <div className="section-label">데이터 처리 원칙</div>
              <div className="setting-group">
                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">100% 온디바이스 변환</div>
                      <div className="row-desc">
                        녹음 음성과 변환 텍스트는 외부 서버 전송 없이 이 Mac
                        내부(Metal GPU)에서만 처리됩니다.
                      </div>
                    </div>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">임시 오디오 즉시 삭제</div>
                      <div className="row-desc">
                        변환 완료 즉시 임시 녹음 파일을 삭제하며, 비정상 종료 시
                        남은 파일도 다음 실행 때 자동 정리합니다.
                      </div>
                    </div>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">클립보드 원상 복원</div>
                      <div className="row-desc">
                        텍스트 입력 전 기존 클립보드를 백업하고 입력 직후 원래
                        내용으로 복원합니다.
                      </div>
                    </div>
                  </div>
                </div>

                <div className="setting-row">
                  <div className="setting-left">
                    <div>
                      <div className="row-title">비밀번호 입력란 보호</div>
                      <div className="row-desc">
                        보안 입력 필드에서는 녹음 시작과 텍스트 자동 입력을 모두
                        차단합니다.
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {activeTab === "licenses" && (
          <div className="tab-pane">
            <div className="pane-topbar">
              <h1 className="pane-heading">오픈소스 라이선스</h1>
            </div>

            {OPEN_SOURCE_CATEGORIES.map((group) => (
              <div key={group.category} className="section-block">
                <div className="section-label">{group.category}</div>
                <div className="setting-group">
                  {group.items.map((item) => (
                    <div key={item.name} className="setting-row license-row">
                      <div className="license-main">
                        <div className="license-title-line">
                          <span className="row-title">{item.name}</span>
                          <span className="license-badge">{item.license}</span>
                        </div>
                        <div className="row-desc">{item.role}</div>
                        <div className="license-meta">
                          <span>{item.version}</span>
                          <span>·</span>
                          <span>{item.copyright}</span>
                        </div>
                      </div>
                      <div className="setting-right">
                        <button
                          type="button"
                          className="soft-btn"
                          onClick={() =>
                            void ttobakApi.openExternalUrl(item.spdxUrl)
                          }
                        >
                          <span>라이선스</span>
                        </button>
                        <button
                          type="button"
                          className="soft-btn"
                          onClick={() =>
                            void ttobakApi.openExternalUrl(item.repoUrl)
                          }
                        >
                          <span>저장소</span>
                          <ExternalLink size={11} />
                        </button>
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </main>

      {/* 단축키 변경 모달 */}
      {shortcutModalOpen && (
        <div
          className="modal-backdrop"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) setShortcutModalOpen(false);
          }}
        >
          <div className="modal-panel">
            <div className="modal-head-row">
              <h3>말하기 단축키</h3>
              <button
                type="button"
                className="modal-close-btn"
                onClick={() => setShortcutModalOpen(false)}
                aria-label="닫기"
              >
                <X size={14} />
              </button>
            </div>
            <p className="modal-desc">
              프리셋을 고르거나 원하는 키 조합(예: <kbd>⌃</kbd>+<kbd>⌥</kbd>+
              <kbd>Space</kbd>)을 직접 누르세요.
            </p>

            <div className="recorder-box">
              <span className="recorder-current">
                {renderKindLabel(draftShortcut.kind)}
              </span>
              <span className="recorder-sub">키보드를 눌러 변경</span>
            </div>

            <div className="preset-chips">
              {(
                [
                  { label: "우측 ⌥", kind: { type: "right_option" } },
                  { label: "좌측 ⌥", kind: { type: "left_option" } },
                  { label: "우측 ⌘", kind: { type: "right_command" } },
                  { label: "우측 ⌃", kind: { type: "right_control" } },
                  { label: "Fn", kind: { type: "fn_key" } },
                ] as { label: string; kind: ShortcutKind }[]
              ).map((preset) => (
                <button
                  key={preset.label}
                  type="button"
                  className={`preset-chip ${
                    draftShortcut.kind.type === preset.kind.type
                      ? "selected"
                      : ""
                  }`}
                  onClick={() =>
                    setDraftShortcut((prev) => ({ ...prev, kind: preset.kind }))
                  }
                >
                  {preset.label}
                </button>
              ))}
            </div>

            <div>
              <div className="hold-title">누르고 유지 시간</div>
              <div className="hold-buttons">
                {[
                  { ms: 0, label: "즉시" },
                  { ms: 500, label: "0.5초" },
                  { ms: 1000, label: "1.0초" },
                ].map((opt) => (
                  <button
                    key={opt.ms}
                    type="button"
                    className={`hold-btn ${
                      draftShortcut.hold_ms === opt.ms ? "selected" : ""
                    }`}
                    onClick={() =>
                      setDraftShortcut((prev) => ({ ...prev, hold_ms: opt.ms }))
                    }
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>

            <div className="modal-actions">
              <button
                type="button"
                className="soft-btn"
                onClick={() => setShortcutModalOpen(false)}
              >
                취소
              </button>
              <button
                type="button"
                className="primary-btn"
                onClick={() => void handleSaveShortcut()}
              >
                저장
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 음성 모델 삭제 확인 모달 */}
      {deleteModalOpen && (
        <div
          className="modal-backdrop"
          onMouseDown={(e) => {
            if (e.target === e.currentTarget && !deletingModel) {
              setDeleteModalOpen(false);
            }
          }}
        >
          <div className="modal-panel">
            <h3>음성 인식 모델 삭제</h3>
            <p className="modal-desc">
              로컬 모델 파일을 삭제해 저장 공간을 확보합니다. 다시 사용하려면
              새로 다운로드해야 합니다.
            </p>

            {hasMultipleVariantsOnDisk && (
              <div>
                <div className="hold-title">삭제 대상</div>
                <div className="delete-target-buttons">
                  {variantsOnDisk.map((item) => (
                    <button
                      key={item.variant}
                      type="button"
                      className={`hold-btn ${
                        deleteTarget === item.variant ? "selected" : ""
                      }`}
                      onClick={() => setDeleteTarget(item.variant)}
                    >
                      {getModelVariantOption(item.variant).shortLabel} (
                      {formatBytes(item.bytes)})
                    </button>
                  ))}
                  <button
                    type="button"
                    className={`hold-btn ${
                      deleteTarget === "all" ? "selected" : ""
                    }`}
                    onClick={() => setDeleteTarget("all")}
                  >
                    전체 ({formatBytes(storage.total_bytes)})
                  </button>
                </div>
              </div>
            )}

            <div className="model-storage-box">
              <div className="model-storage-meta">
                <span className="model-storage-label">확보 용량</span>
                <strong className="model-storage-size">
                  {deleteTarget === "all"
                    ? formatBytes(storage.total_bytes)
                    : formatBytes(
                        variantsOnDisk.find((v) => v.variant === deleteTarget)
                          ?.bytes ?? 0,
                      )}
                </strong>
              </div>
              <div className="model-storage-path-row">
                <code className="model-storage-path">{storage.models_dir}</code>
                <button
                  type="button"
                  className="ghost-refresh-btn"
                  onClick={() => void ttobakApi.revealModelsFolder()}
                >
                  <FolderOpen size={12} /> 폴더 열기
                </button>
              </div>
            </div>

            <div className="modal-actions">
              <button
                type="button"
                className="soft-btn"
                disabled={deletingModel}
                onClick={() => setDeleteModalOpen(false)}
              >
                취소
              </button>
              <button
                type="button"
                className="danger-solid-btn"
                disabled={deletingModel}
                onClick={() => void handleConfirmDeleteModel()}
              >
                <Trash2 size={12.5} />
                <span>{deletingModel ? "삭제 중..." : "삭제"}</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {/* 첫 사용 사전 안내 모달 */}
      {showFirstRunModal && (
        <div className="modal-backdrop">
          <div className="modal-panel">
            <div className="notice-badge">
              <TtobakEmblem size={22} />
              <ShieldCheck size={15} />
              <span>첫 사용 안내</span>
            </div>
            <h3>내 목소리는 이 Mac 안에서만 처리됩니다</h3>
            <ul className="notice-list">
              <li>
                <strong>온디바이스 변환:</strong> 외부 서버 전송 없이 이 Mac
                내부에서만 변환됩니다.
              </li>
              <li>
                <strong>임시 파일 즉시 삭제:</strong> 변환 완료 즉시 임시 오디오
                파일을 삭제합니다.
              </li>
              <li>
                <strong>비밀번호 입력란 보호:</strong> 보안 입력 칸에서는 녹음과
                자동 입력을 차단합니다.
              </li>
            </ul>
            <div className="modal-actions">
              <button
                type="button"
                className="primary-btn full"
                onClick={() => void ttobakApi.acceptFirstNotice()}
              >
                시작하기
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
