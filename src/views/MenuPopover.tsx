import React, { useEffect } from "react";
import { Loader2, Mic, Settings, RefreshCw, Power, X } from "lucide-react";
import type { AppSnapshot } from "../types";
import { ttobakApi } from "../api";
import { TtobakEmblem } from "../components/TtobakEmblem";

interface Props {
  snapshot: AppSnapshot;
}

export const MenuPopover: React.FC<Props> = ({ snapshot }) => {
  const isRecording = snapshot.session_phase.phase === "recording";
  const isTranscribing = snapshot.session_phase.phase === "transcribing";
  const isSessionActive = isRecording || isTranscribing;
  const readiness = snapshot.model_readiness;

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === ",") {
        e.preventDefault();
        void ttobakApi.openSettings();
        return;
      }
      if (e.metaKey && e.key.toLowerCase() === "q") {
        e.preventDefault();
        void ttobakApi.quitApp();
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        if (isSessionActive) {
          void ttobakApi.cancelSession();
        } else {
          void ttobakApi.hideMenuPopover();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isSessionActive]);

  const renderStatusChip = () => {
    if (isRecording) {
      return (
        <span className="popover-status-chip live">
          <span className="status-dot live" />
          녹음 중
        </span>
      );
    }
    if (isTranscribing) {
      return (
        <span className="popover-status-chip busy">
          <Loader2 size={11} className="spin" />
          변환 중
        </span>
      );
    }

    const permsOk =
      snapshot.permissions.microphone === "granted" &&
      snapshot.permissions.accessibility === "granted";

    if (readiness.state === "ready" && permsOk) {
      return (
        <span className="popover-status-chip">
          <span className="status-dot ok" />
          준비됨
        </span>
      );
    }
    if (readiness.state === "ready" && !permsOk) {
      return (
        <span className="popover-status-chip warn">
          <span className="status-dot warn" />
          권한 필요
        </span>
      );
    }
    if (readiness.state === "downloading") {
      return (
        <span className="popover-status-chip busy">
          <Loader2 size={11} className="spin" />
          모델 {Math.round(readiness.progress * 100)}%
        </span>
      );
    }
    if (readiness.state === "loading") {
      return (
        <span className="popover-status-chip busy">
          <Loader2 size={11} className="spin" />
          엔진 준비 중
        </span>
      );
    }
    return (
      <span className="popover-status-chip warn">
        <span className="status-dot warn" />
        모델 필요
      </span>
    );
  };

  return (
    <div
      className="popover-stage"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) {
          void ttobakApi.hideMenuPopover();
        }
      }}
    >
      <div className="popover-shell">
        <div className="popover-header">
          <div className="popover-brand-left">
            <TtobakEmblem size={24} active={isRecording} />
            <span className="popover-brand-title">또박</span>
          </div>
          {renderStatusChip()}
        </div>

        {readiness.state === "downloading" && (
          <div className="popover-mini-progress" aria-hidden="true">
            <div
              className="popover-mini-progress-fill"
              style={{
                width: `${Math.max(4, Math.round(readiness.progress * 100))}%`,
              }}
            />
          </div>
        )}

        <div className="popover-action-stack">
          <button
            type="button"
            className={`popover-primary-btn ${isRecording ? "recording" : ""}`}
            disabled={isTranscribing}
            onClick={() => void ttobakApi.toggleRecording()}
          >
            <span className="popover-primary-left">
              {isRecording ? (
                <span className="popover-live-bars" aria-hidden="true">
                  <span />
                  <span />
                  <span />
                </span>
              ) : isTranscribing ? (
                <Loader2 size={14} className="spin" />
              ) : (
                <Mic size={14} strokeWidth={2.2} />
              )}
              <span>
                {isRecording
                  ? "말하기 완료"
                  : isTranscribing
                    ? "변환 중..."
                    : "말하기 시작"}
              </span>
            </span>
            <span className="popover-shortcut-hint">
              {snapshot.shortcut_label}
            </span>
          </button>

          {isSessionActive && (
            <button
              type="button"
              className="popover-cancel-btn"
              onClick={() => void ttobakApi.cancelSession()}
            >
              <span className="popover-primary-left">
                <X size={13} strokeWidth={2.1} />
                <span>음성 입력 취소</span>
              </span>
              <span className="popover-shortcut-hint">Esc</span>
            </button>
          )}
        </div>

        <div className="popover-divider" />

        <div className="popover-menu-list">
          <button
            type="button"
            className="popover-menu-item"
            onClick={() => void ttobakApi.openSettings()}
          >
            <span className="popover-menu-item-left">
              <Settings size={14} strokeWidth={1.9} />
              <span>설정...</span>
            </span>
            <span className="popover-menu-kbd">⌘,</span>
          </button>

          <button
            type="button"
            className="popover-menu-item"
            onClick={() => {
              void ttobakApi.openSettings();
              void ttobakApi.checkAppUpdate();
            }}
          >
            <span className="popover-menu-item-left">
              <RefreshCw size={14} strokeWidth={1.9} />
              <span>업데이트 확인...</span>
            </span>
            {snapshot.app_update.status === "available" && (
              <span className="popover-update-pill">
                v{snapshot.app_update.latest_version}
              </span>
            )}
          </button>
        </div>

        <div className="popover-divider" />

        <div className="popover-menu-list">
          <button
            type="button"
            className="popover-menu-item"
            onClick={() => void ttobakApi.quitApp()}
          >
            <span className="popover-menu-item-left">
              <Power size={14} strokeWidth={1.9} />
              <span>또박 종료</span>
            </span>
            <span className="popover-menu-kbd">⌘Q</span>
          </button>
        </div>
      </div>
    </div>
  );
};
