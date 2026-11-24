import { useState, useEffect } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { DashboardView } from "./components/DashboardView";
import { OverlayBarrierView } from "./components/OverlayBarrierView";
import "./App.css";

function getInitialWindowLabel(): string {
  try {
    return getCurrentWebviewWindow().label;
  } catch {
    // In normal browser preview, default to main
    return "main";
  }
}

export function App() {
  const [windowLabel] = useState<string>(getInitialWindowLabel);

  useEffect(() => {
    document.body.dataset.window = windowLabel;
  }, [windowLabel]);

  if (windowLabel.startsWith("overlay")) {
    const isSecondary = windowLabel !== "overlay";
    return <OverlayBarrierView isSecondary={isSecondary} />;
  }

  return <DashboardView />;
}

export default App;
