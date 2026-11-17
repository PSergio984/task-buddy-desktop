import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Play, Square, Shield, Clock, Zap, Laptop, Activity, Music } from "lucide-react";

interface SessionState {
  is_focus_active: boolean;
  task_title: string;
  duration_minutes: number;
  elapsed_seconds: number;
  is_overlay_visible: boolean;
}

interface ActiveWindowInfo {
  title: string;
  process_name: string;
  is_empty: boolean;
}

interface MediaStatus {
  has_active_media: boolean;
  is_playing: boolean;
  title: string;
  artist: string;
  source_app_id: string;
}

export const DashboardView: React.FC = () => {
  const [session, setSession] = useState<SessionState>({
    is_focus_active: false,
    task_title: "Deep Work",
    duration_minutes: 25,
    elapsed_seconds: 0,
    is_overlay_visible: false,
  });
  const [taskInput, setTaskInput] = useState("");
  const [osInfo, setOsInfo] = useState<{ window: ActiveWindowInfo; media: MediaStatus }>({
    window: { title: "", process_name: "", is_empty: true },
    media: { has_active_media: false, is_playing: false, title: "", artist: "", source_app_id: "" },
  });

  const refreshStatus = async () => {
    try {
      const res = await invoke<SessionState>("get_session_status");
      setSession(res);
      if (res.task_title && res.task_title !== "Deep Work") {
        setTaskInput(res.task_title);
      }
    } catch (err) {
      console.error("Failed to fetch session status:", err);
    }
  };

  const pollOsInfo = async () => {
    try {
      const [win, med] = await invoke<[ActiveWindowInfo, MediaStatus]>("get_live_os_info");
      setOsInfo({ window: win, media: med });
    } catch {
      // Ignore during initial load
    }
  };

  useEffect(() => {
    refreshStatus();
    pollOsInfo();

    const interval = setInterval(pollOsInfo, 1500);

    const unlistenPromise = listen<SessionState>("session-changed", (event) => {
      setSession(event.payload);
    });

    return () => {
      clearInterval(interval);
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const handleStartSession = async (minutes: number) => {
    try {
      const res = await invoke<SessionState>("start_focus_session", {
        taskTitle: taskInput.trim() || "Deep Work Session",
        durationMinutes: minutes,
      });
      setSession(res);
    } catch (err) {
      console.error("Failed to start session:", err);
    }
  };

  const handleEndSession = async () => {
    try {
      const res = await invoke<SessionState>("end_focus_session");
      setSession(res);
    } catch (err) {
      console.error("Failed to end session:", err);
    }
  };

  const handleTriggerBarrier = async () => {
    try {
      await invoke("trigger_barrier");
    } catch (err) {
      console.error("Failed to trigger barrier:", err);
    }
  };

  return (
    <div className="flex flex-col h-screen w-full bg-slate-950 text-slate-100 select-none">
      {/* Top Header */}
      <header className="flex items-center justify-between px-5 py-3.5 border-b border-slate-800/80 bg-slate-900/40">
        <div className="flex items-center gap-2.5">
          <div className="p-1.5 rounded-lg bg-emerald-500/10 text-emerald-400 border border-emerald-500/20">
            <Zap className="w-4 h-4" />
          </div>
          <div>
            <h1 className="text-sm font-semibold tracking-tight text-white leading-none">
              Task Buddy
            </h1>
            <span className="text-[10px] text-slate-400 font-medium">Desktop Daemon v0.1</span>
          </div>
        </div>

        <div className="flex items-center gap-1.5">
          <span
            className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium border ${
              session.is_focus_active
                ? "bg-amber-500/10 text-amber-400 border-amber-500/30 animate-pulse"
                : "bg-emerald-500/10 text-emerald-400 border-emerald-500/30"
            }`}
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                session.is_focus_active ? "bg-amber-400" : "bg-emerald-400"
              }`}
            />
            {session.is_focus_active ? "Focus Mode Active" : "Free Time (Standby)"}
          </span>
        </div>
      </header>

      {/* Main Body */}
      <main className="flex-1 p-5 flex flex-col justify-between overflow-y-auto space-y-4">
        <div className="space-y-4">
          {/* Active Task Card */}
          <div className="p-4 rounded-xl bg-slate-900/60 border border-slate-800 space-y-2.5">
            <label className="text-xs font-semibold text-slate-400 uppercase tracking-wider block">
              Declared Hourly Task
            </label>
            <input
              type="text"
              disabled={session.is_focus_active}
              value={taskInput}
              onChange={(e) => setTaskInput(e.target.value)}
              placeholder="e.g. Implement Rust WinRT daemon"
              className="w-full px-3 py-2 text-sm bg-slate-950 border border-slate-700/80 rounded-lg text-white placeholder-slate-500 focus:outline-none focus:border-emerald-500 disabled:opacity-70 disabled:cursor-not-allowed"
            />
            <p className="text-[11px] text-slate-500 leading-normal">
              Digital barriers will verify active tutorials and window titles against this target.
            </p>
          </div>

          {/* Session Controller */}
          <div className="p-4 rounded-xl bg-slate-900/60 border border-slate-800 space-y-3">
            <span className="text-xs font-semibold text-slate-400 uppercase tracking-wider block">
              Focus Controls
            </span>

            {!session.is_focus_active ? (
              <div className="grid grid-cols-2 gap-2.5">
                <button
                  onClick={() => handleStartSession(25)}
                  className="flex items-center justify-center gap-2 px-3 py-2.5 bg-emerald-600 hover:bg-emerald-500 active:bg-emerald-700 text-white font-medium text-xs rounded-lg transition-colors shadow-sm cursor-pointer"
                >
                  <Play className="w-3.5 h-3.5" />
                  <span>Start 25m Block</span>
                </button>
                <button
                  onClick={() => handleStartSession(50)}
                  className="flex items-center justify-center gap-2 px-3 py-2.5 bg-slate-800 hover:bg-slate-700 active:bg-slate-850 text-white font-medium text-xs rounded-lg transition-colors border border-slate-700/80 cursor-pointer"
                >
                  <Play className="w-3.5 h-3.5" />
                  <span>Start 50m Block</span>
                </button>
              </div>
            ) : (
              <div className="space-y-3">
                <div className="flex items-center justify-between p-3 rounded-lg bg-amber-500/10 border border-amber-500/20 text-amber-300">
                  <div className="flex items-center gap-2">
                    <Clock className="w-4 h-4 text-amber-400" />
                    <span className="text-xs font-medium">Session in Progress</span>
                  </div>
                  <span className="text-xs font-mono font-bold">
                    {session.duration_minutes}:00 target
                  </span>
                </div>

                <button
                  onClick={handleEndSession}
                  className="w-full flex items-center justify-center gap-2 px-3 py-2.5 bg-red-600/20 hover:bg-red-600/30 text-red-400 border border-red-500/30 font-medium text-xs rounded-lg transition-colors cursor-pointer"
                >
                  <Square className="w-3.5 h-3.5" />
                  <span>End Session (Return to Free Time)</span>
                </button>
              </div>
            )}
          </div>

          {/* Live OS Detection Diagnostics */}
          <div className="p-3.5 rounded-xl bg-slate-900/40 border border-slate-800/80 space-y-2">
            <span className="text-[10px] font-semibold text-slate-400 uppercase tracking-wider block flex items-center gap-1.5">
              <Activity className="w-3 h-3 text-cyan-400" />
              <span>Native Windows Telemetry (0.0% CPU)</span>
            </span>

            <div className="space-y-1.5 text-xs">
              <div className="flex items-center justify-between">
                <span className="text-slate-400">Foreground Process:</span>
                <span className="font-mono text-emerald-400 truncate max-w-[200px]">
                  {osInfo.window.process_name || "idle / none"}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-slate-400">WinRT Media:</span>
                <span className="text-slate-300 flex items-center gap-1 truncate max-w-[200px]">
                  <Music className="w-3 h-3 text-blue-400 shrink-0" />
                  <span className="truncate">
                    {osInfo.media.has_active_media
                      ? `${osInfo.media.is_playing ? "▶ Playing:" : "⏸ Paused:"} ${osInfo.media.title || "Unknown"}`
                      : "No active media session"}
                  </span>
                </span>
              </div>
            </div>
          </div>

          {/* Test Friction Barrier */}
          <div className="p-3.5 rounded-xl bg-slate-900/30 border border-slate-800/80 flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <Shield className="w-4 h-4 text-amber-400" />
              <div>
                <div className="text-xs font-medium text-slate-200">Friction Barrier Preview</div>
                <div className="text-[10px] text-slate-500">Test the 6-second hold-to-bypass dial</div>
              </div>
            </div>
            <button
              onClick={handleTriggerBarrier}
              className="px-2.5 py-1 text-xs font-medium bg-slate-800 hover:bg-slate-700 text-slate-300 rounded border border-slate-700 transition-colors cursor-pointer"
            >
              Test
            </button>
          </div>
        </div>

        {/* Footer info */}
        <footer className="pt-4 border-t border-slate-800/60 flex items-center justify-between text-[11px] text-slate-500">
          <div className="flex items-center gap-1.5">
            <Laptop className="w-3.5 h-3.5" />
            <span>Windows Native Daemon Active</span>
          </div>
          <span>Tray Minimized</span>
        </footer>
      </main>
    </div>
  );
};
