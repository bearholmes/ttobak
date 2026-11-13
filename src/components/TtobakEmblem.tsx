import React from "react";
import logoUrl from "../assets/ttobak-logo.png";

interface EmblemProps {
  size?: number;
  active?: boolean;
  className?: string;
}

/** 또박(Ttobak) 공식 브랜드 로고 컴포넌트 (최적화된 256px Retina 스쿼클 에셋) */
export const TtobakEmblem: React.FC<EmblemProps> = ({
  size = 56,
  active = false,
  className = "",
}) => {
  return (
    <span
      className={`ttobak-emblem-wrap ${active ? "is-active" : ""} ${className}`.trim()}
      style={{ width: size, height: size }}
      aria-hidden="true"
    >
      <img
        src={logoUrl}
        alt="또박 로고"
        width={size}
        height={size}
        draggable={false}
        className="ttobak-emblem-img"
      />
    </span>
  );
};
