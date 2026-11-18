import { useState } from "react";
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

  if (windowLabel === "overlay") {
    return <OverlayBarrierView />;
  }

  return <DashboardView />;
}

export default App;
