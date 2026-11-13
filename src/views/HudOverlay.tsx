import React, { useState } from "react";
import { Copy, Check, X, Lock, AlertTriangle } from "lucide-react";
import type { AppSnapshot } from "../types";
import { ttobakApi } from "../api";

interface Props {
  snapshot: AppSnapshot;
}

function formatDuration(totalSecs: number): string {
  const mins = Math.floor(totalSecs / 60);
  const secs = totalSecs % 60;
  return `${String(mins).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
}

const BAR_WEIGHTS = [0.4, 0.68, 0.88, 0.56, 1.0, 0.72, 0.92, 0.62, 0.42];

export const HudOverlay: React.FC<Props> = ({ snapshot }) => {
  const [copied, setCopied] = useState(false);
  const phase = snapshot.session_phase;

  if (phase.phase === "idle" || phase.phase === "first_run_notice") {
    return null;
  }

  if (phase.phase === "recording") {
    const level = Math.max(0.08, Math.min(1, phase.rms_level));
    return (
      <div className="hud-stage">
        <div className="hud-capsule">
          <span className="hud-timer-only">
            {formatDuration(phase.elapsed_secs)}
          </span>
          <div className="hud-waveform" aria-hidden="true">
            {BAR_WEIGHTS.map((w, idx) => {
              const h = Math.round(5 + level * w * 18);
              return (
                <span
                  key={idx}
                  className="hud-wave-bar"
                  style={{ height: `${h}px` }}
                />
              );
            })}
          </div>
        </div>
      </div>
    );
  }

  if (phase.phase === "transcribing") {
    return (
      <div className="hud-stage">
        <div
          className="hud-capsule transcribing"
          role="progressbar"
          aria-label="음성 변환 중"
        >
          <div className="hud-progress-track">
            <div className="hud-progress-indeterminate" />
          </div>
        </div>
      </div>
    );
  }

  if (phase.phase === "blocked_secure_input") {
    return (
      <div className="hud-stage">
        <div className="hud-capsule secure-blocked">
          <Lock size={16} />
          <div className="hud-secure-text">{phase.message}</div>
        </div>
      </div>
    );
  }

  if (phase.phase === "failed") {
    return (
      <div className="hud-stage">
        <div className="hud-capsule secure-blocked">
          <AlertTriangle size={16} />
          <div className="hud-secure-text">{phase.message}</div>
        </div>
      </div>
    );
  }

  if (phase.phase === "recovery_card") {
    const handleCopy = async () => {
      setCopied(true);
      await ttobakApi.copyRecoveryTranscript();
      setTimeout(() => setCopied(false), 800);
    };

    return (
      <div className="hud-stage interactive">
        <div className="recovery-card">
          <div className="recovery-top">
            <span className="recovery-reason">{phase.reason}</span>
            <button
              type="button"
              className="recovery-close"
              onClick={() => void ttobakApi.dismissRecovery()}
              aria-label="닫기"
            >
              <X size={15} />
            </button>
          </div>
          <div className="recovery-transcript">{phase.transcript}</div>
          <div className="recovery-actions">
            <div className="recovery-progress-track">
              <div
                className="recovery-progress-fill"
                style={{
                  width: `${Math.round(phase.remaining_fraction * 100)}%`,
                }}
              />
            </div>
            <button
              type="button"
              className="recovery-copy-btn"
              onClick={() => void handleCopy()}
            >
              {copied ? <Check size={14} /> : <Copy size={14} />}
              <span>{copied ? "복사됨" : "복사하기"}</span>
            </button>
          </div>
        </div>
      </div>
    );
  }

  return null;
};
