import React, { useEffect, useMemo } from "react";
import { useTtobakSnapshot } from "./api";
import { MenuPopover } from "./views/MenuPopover";
import { HudOverlay } from "./views/HudOverlay";
import { SettingsWindow } from "./views/SettingsWindow";

type RouteView = "menu" | "hud" | "settings";

function detectView(): RouteView {
  const hash = window.location.hash.replace("#", "").toLowerCase();
  if (hash === "menu" || hash === "hud" || hash === "settings") {
    return hash;
  }
  return "settings";
}

export const App: React.FC = () => {
  const { snapshot } = useTtobakSnapshot();
  const view = useMemo(detectView, []);

  useEffect(() => {
    document.documentElement.dataset.view = view;
  }, [view]);

  if (view === "menu") {
    return <MenuPopover snapshot={snapshot} />;
  }
  if (view === "hud") {
    return <HudOverlay snapshot={snapshot} />;
  }
  return <SettingsWindow snapshot={snapshot} />;
};
