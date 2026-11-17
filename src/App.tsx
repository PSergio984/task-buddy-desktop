import { useEffect, useState } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { DashboardView } from "./components/DashboardView";
import { OverlayBarrierView } from "./components/OverlayBarrierView";
import "./App.css";

export function App() {
  const [windowLabel, setWindowLabel] = useState<string>("main");

  useEffect(() => {
    try {
      const win = getCurrentWebviewWindow();
      setWindowLabel(win.label);
    } catch {
      // In normal browser preview, default to main
      setWindowLabel("main");
    }
  }, []);

  if (windowLabel === "overlay") {
    return <OverlayBarrierView />;
  }

  return <DashboardView />;
}

export default App;
