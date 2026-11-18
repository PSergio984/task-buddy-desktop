import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Play, Square, Shield, Clock, Zap, Laptop, Activity, Music, Plus, Trash2, ListFilter, Cloud, Sparkles, Pin, PinOff, Eye } from "lucide-react";

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

interface ProcessRule {
  id: number;
  process_name: string;
  category: string;
  created_at: string;
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
  const [authToken, setAuthToken] = useState(() => localStorage.getItem("backend_auth_token") || "");
  const [committedToken, setCommittedToken] = useState(() => localStorage.getItem("backend_auth_token") || "");
  const [actionError, setActionError] = useState<string | null>(null);
  const [groqApiKey, setGroqApiKey] = useState(() => localStorage.getItem("groq_api_key") || "");
  const [rules, setRules] = useState<ProcessRule[]>([]);
  const [newProcessName, setNewProcessName] = useState("");
  const [newCategory, setNewCategory] = useState<"whitelist" | "blacklist">("whitelist");
  const [activeTab, setActiveTab] = useState<"focus" | "rules">("focus");
  const [showHud, setShowHud] = useState(false);
  const [hudPinned, setHudPinned] = useState(false);
  const [isStartingSession, setIsStartingSession] = useState(false);
  const [sseStatus, setSseStatus] = useState<"idle" | "connecting" | "connected" | "error" | "auth_error">("idle");

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

  const refreshRules = async () => {
    try {
      const list = await invoke<ProcessRule[]>("get_local_rules");
      setRules(list);
    } catch (err) {
      console.error("Failed to fetch rules:", err);
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

  const syncRemote = async () => {
    if (!committedToken.trim()) return;
    try {
      const res = await invoke<any>("sync_remote_session", { token: committedToken.trim() });
      if (res?.current_phase?.minutes) {
        setSession((prev) => ({
          ...prev,
          duration_minutes: res.current_phase.minutes,
        }));
      }
    } catch {
      // Best-effort remote sync
    }
  };

  const authTokenRef = useRef(committedToken);
  authTokenRef.current = committedToken;
  const hudPinnedRef = useRef(hudPinned);
  hudPinnedRef.current = hudPinned;
  const glanceTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const actionErrorTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const setFeedbackError = (msg: string | null) => {
    if (actionErrorTimeoutRef.current) {
      clearTimeout(actionErrorTimeoutRef.current);
      actionErrorTimeoutRef.current = null;
    }
    setActionError(msg);
    if (msg) {
      actionErrorTimeoutRef.current = setTimeout(() => {
        setActionError(null);
        actionErrorTimeoutRef.current = null;
      }, 4000);
    }
  };

  const triggerHudGlance = () => {
    setShowHud(true);
    if (glanceTimeoutRef.current) {
      clearTimeout(glanceTimeoutRef.current);
      glanceTimeoutRef.current = null;
    }
    if (!hudPinnedRef.current) {
      glanceTimeoutRef.current = setTimeout(() => {
        if (!hudPinnedRef.current) {
          setShowHud(false);
        }
      }, 2500);
    }
  };

  const handleAuthTokenChange = (token: string) => {
    setAuthToken(token);
  };

  const handleAuthTokenCommit = () => {
    const trimmed = authToken.trim();
    localStorage.setItem("backend_auth_token", trimmed);
    setCommittedToken(trimmed);
  };

  // Static event listeners setup (runs once on mount)
  useEffect(() => {
    refreshStatus();
    refreshRules();

    const savedGroqKey = localStorage.getItem("groq_api_key");
    if (savedGroqKey) {
      invoke("set_groq_api_key", { key: savedGroqKey.trim() || null }).catch(console.error);
    }

    const interval = setInterval(pollOsInfo, 1500);

    const unlistenSessionPromise = listen<SessionState>("session-changed", (event) => {
      setSession(event.payload);
    });

    const unlistenHudPromise = listen("toggle-task-hud", () => {
      triggerHudGlance();
    });

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.ctrlKey && e.altKey && e.code === "Space") {
        e.preventDefault();
        triggerHudGlance();
      }
    };

    window.addEventListener("keydown", handleKeyDown);

    return () => {
      clearInterval(interval);
      if (glanceTimeoutRef.current) {
        clearTimeout(glanceTimeoutRef.current);
      }
      if (actionErrorTimeoutRef.current) {
        clearTimeout(actionErrorTimeoutRef.current);
      }
      window.removeEventListener("keydown", handleKeyDown);
      unlistenSessionPromise.then((unlisten) => unlisten());
      unlistenHudPromise.then((unlisten) => unlisten());
    };
  }, []);

  // Dynamic Focus Session Countdown Ticker
  useEffect(() => {
    if (!session.is_focus_active) return;
    const ticker = setInterval(() => {
      setSession((prev) => ({
        ...prev,
        elapsed_seconds: (prev.elapsed_seconds || 0) + 1,
      }));
    }, 1000);
    return () => clearInterval(ticker);
  }, [session.is_focus_active]);

  // Persistent Server-Sent Events (SSE) Push Synchronization Stream
  useEffect(() => {
    if (!committedToken || !committedToken.trim()) {
      setSseStatus("idle");
      return;
    }

    syncRemote();
    const controller = new AbortController();
    let isCancelled = false;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

    const connectSse = async () => {
      if (isCancelled) return;
      setSseStatus("connecting");
      try {
        const response = await fetch("http://localhost:8000/api/v1/game/sessions/stream", {
          headers: {
            Authorization: `Bearer ${committedToken.trim()}`,
          },
          signal: controller.signal,
        });

        if (response.status >= 400 && response.status < 500) {
          setSseStatus("auth_error");
          return; // Non-retryable client / authentication failure
        }

        if (!response.ok || !response.body) {
          setSseStatus("error");
          if (!isCancelled) {
            reconnectTimer = setTimeout(connectSse, 5000);
          }
          return;
        }

        setSseStatus("connected");
        const reader = response.body.getReader();
        const decoder = new TextDecoder("utf-8");
        let buffer = "";

        while (!isCancelled) {
          const { done, value } = await reader.read();
          if (done) break;
          buffer += decoder.decode(value, { stream: true });

          // Retain trailing \r in case next chunk delivers \n
          let toProcess = buffer;
          let leftover = "";
          if (toProcess.endsWith("\r")) {
            toProcess = toProcess.slice(0, -1);
            leftover = "\r";
          }
          // Normalize CRLF (\r\n) and bare CR (\r) into standard \n
          const normalized = toProcess.replace(/\r\n/g, "\n").replace(/\r/g, "\n");
          const lines = normalized.split("\n\n");
          buffer = (lines.pop() || "") + leftover;

          for (const chunk of lines) {
            for (const line of chunk.split("\n")) {
              if (line.startsWith("data:")) {
                try {
                  const stateUpdate = await invoke<any>("sync_remote_session", {
                    token: committedToken.trim(),
                  });
                  if (stateUpdate?.current_phase?.minutes) {
                    setSession((prev) => ({
                      ...prev,
                      duration_minutes: stateUpdate.current_phase.minutes,
                    }));
                  }
                } catch {
                  // Silent decode parse
                }
              }
            }
          }
        }
        if (!isCancelled) {
          setSseStatus("connecting");
          reconnectTimer = setTimeout(connectSse, 3000);
        }
      } catch (err: any) {
        if (err.name !== "AbortError" && !isCancelled) {
          setSseStatus("error");
          reconnectTimer = setTimeout(connectSse, 5000);
        }
      }
    };

    connectSse();

    return () => {
      isCancelled = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      controller.abort();
    };
  }, [committedToken]);

  const handleGroqKeyChange = async (key: string) => {
    setGroqApiKey(key);
    localStorage.setItem("groq_api_key", key);
    try {
      await invoke("set_groq_api_key", { key: key.trim() || null });
    } catch (err) {
      console.error("Failed to update Groq API key:", err);
    }
  };

  const handleStartSession = async (minutes: number) => {
    if (isStartingSession) return;
    setIsStartingSession(true);
    try {
      const res = await invoke<SessionState>("start_focus_session", {
        taskTitle: taskInput.trim() || "Deep Work Session",
        durationMinutes: minutes,
        token: committedToken.trim() || null,
      });
      setSession(res);
      setFeedbackError(null);
    } catch (err: any) {
      console.error("Failed to start session:", err);
      setFeedbackError(`Failed to start session: ${err}`);
    } finally {
      setIsStartingSession(false);
    }
  };

  const handleEndSession = async () => {
    try {
      const res = await invoke<SessionState>("end_focus_session");
      setSession(res);
      setFeedbackError(null);
    } catch (err: any) {
      console.error("Failed to end session:", err);
      setFeedbackError(`Failed to end session: ${err}`);
    }
  };

  const handleAddRule = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!newProcessName.trim()) return;

    try {
      await invoke("add_local_rule", {
        processName: newProcessName.trim(),
        category: newCategory,
      });
      setNewProcessName("");
      refreshRules();
      setFeedbackError(null);
    } catch (err: any) {
      console.error("Failed to add rule:", err);
      setFeedbackError(`Failed to add rule: ${err}`);
    }
  };

  const handleDeleteRule = async (id: number) => {
    try {
      await invoke("delete_local_rule", { id });
      refreshRules();
      setFeedbackError(null);
    } catch (err: any) {
      console.error("Failed to delete rule:", err);
      setFeedbackError(`Failed to delete rule: ${err}`);
    }
  };

  const handleTriggerBarrier = async () => {
    try {
      await invoke("trigger_barrier");
    } catch (err) {
      console.error("Failed to trigger barrier:", err);
    }
  };

  const totalSec = (session.duration_minutes || 25) * 60;
  const remSec = Math.max(0, totalSec - (session.elapsed_seconds || 0));
  const remMin = Math.floor(remSec / 60);
  const remSecMod = remSec % 60;
  const displayRemaining = `${remMin}:${remSecMod.toString().padStart(2, "0")}`;

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

        <div className="flex items-center gap-2">
          <button
            onClick={triggerHudGlance}
            title="Click or press Ctrl+Alt+Space for quick glance"
            className="text-[10px] text-slate-400 hover:text-slate-200 bg-slate-800/80 hover:bg-slate-800 px-2 py-1 rounded border border-slate-700/60 transition-colors flex items-center gap-1 cursor-pointer"
          >
            <Eye className="w-3 h-3 text-purple-400" />
            <span>Ctrl+Alt+Space</span>
          </button>
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

      {/* Ambient Task Glance HUD (Ctrl+Alt+Space) */}
      {(showHud || hudPinned) && (
        <div className="mx-5 mt-3 p-3 rounded-xl bg-purple-950/80 border border-purple-500/40 backdrop-blur-md shadow-lg flex items-center justify-between text-xs animate-in fade-in slide-in-from-top duration-200">
          <div className="flex items-center gap-2.5">
            <div className="p-1.5 rounded-md bg-purple-500/20 text-purple-300">
              <Eye className="w-3.5 h-3.5" />
            </div>
            <div>
              <span className="text-[10px] text-purple-300 font-semibold uppercase tracking-wider block">
                Active Focus Target
              </span>
              <span className="font-medium text-white text-xs truncate max-w-[220px] block">
                {session.task_title || "Deep Work"}
              </span>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <span className="font-mono text-purple-200 text-xs font-semibold px-2 py-0.5 rounded bg-purple-900/60 border border-purple-700/50">
              {displayRemaining}
            </span>
            <button
              onClick={() => {
                if (hudPinned) setShowHud(false);
                setHudPinned(!hudPinned);
              }}
              title={hudPinned ? "Unpin HUD" : "Pin HUD"}
              className={`p-1.5 rounded transition-colors cursor-pointer ${
                hudPinned
                  ? "bg-purple-600 text-white"
                  : "text-purple-300 hover:text-white hover:bg-purple-800/40"
              }`}
            >
              {hudPinned ? <PinOff className="w-3 h-3" /> : <Pin className="w-3 h-3" />}
            </button>
          </div>
        </div>
      )}

      {/* Navigation Tabs */}
      <div className="flex border-b border-slate-800/60 bg-slate-900/20 px-5 text-xs">
        <button
          onClick={() => setActiveTab("focus")}
          className={`py-2 px-3 border-b-2 font-medium transition-colors ${
            activeTab === "focus"
              ? "border-emerald-500 text-emerald-400"
              : "border-transparent text-slate-400 hover:text-slate-200"
          }`}
        >
          Focus Session
        </button>
        <button
          onClick={() => setActiveTab("rules")}
          className={`py-2 px-3 border-b-2 font-medium transition-colors flex items-center gap-1.5 ${
            activeTab === "rules"
              ? "border-emerald-500 text-emerald-400"
              : "border-transparent text-slate-400 hover:text-slate-200"
          }`}
        >
          <ListFilter className="w-3.5 h-3.5" />
          <span>Rules ({rules.length})</span>
        </button>
      </div>

      {/* Main Body */}
      <main className="flex-1 p-5 flex flex-col justify-between overflow-y-auto space-y-4">
        {/* Error Notification rendered on both Focus and Rules tabs */}
        {actionError && (
          <div className="p-3 rounded-xl bg-red-500/10 border border-red-500/20 text-red-400 text-xs flex items-center justify-between">
            <span>{actionError}</span>
            <button
              onClick={() => setFeedbackError(null)}
              className="text-red-400 hover:text-red-200 text-xs px-1 cursor-pointer"
            >
              ✕
            </button>
          </div>
        )}

        {activeTab === "focus" ? (
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

            {/* Backend Sync Token Input */}
            <div className="p-3.5 rounded-xl bg-slate-900/30 border border-slate-800/80 space-y-1.5">
              <div className="flex items-center justify-between text-xs">
                <span className="font-medium text-slate-300 flex items-center gap-1.5">
                  <Cloud className="w-3.5 h-3.5 text-blue-400" />
                  <span>Task Buddy API Sync</span>
                </span>
                <span
                  className={`text-[10px] ${
                    !committedToken
                      ? "text-emerald-400"
                      : sseStatus === "connected"
                      ? "text-emerald-400"
                      : sseStatus === "connecting"
                      ? "text-amber-400"
                      : sseStatus === "auth_error"
                      ? "text-red-400"
                      : "text-rose-400"
                  }`}
                >
                  {!committedToken
                    ? "Offline SQLite Active"
                    : sseStatus === "connected"
                    ? "SSE Web Sync Active"
                    : sseStatus === "connecting"
                    ? "Connecting SSE Sync..."
                    : sseStatus === "auth_error"
                    ? "Sync Auth Invalid"
                    : "Sync Disconnected"}
                </span>
              </div>
              <input
                type="password"
                disabled={session.is_focus_active}
                value={authToken}
                onChange={(e) => handleAuthTokenChange(e.target.value)}
                onBlur={handleAuthTokenCommit}
                onKeyDown={(e) => {
                  if (e.key === "Enter") handleAuthTokenCommit();
                }}
                placeholder="Paste Bearer Token to sync streaks (optional)"
                className="w-full px-2.5 py-1.5 text-xs bg-slate-950 border border-slate-800 rounded text-slate-300 placeholder-slate-600 focus:outline-none focus:border-blue-500"
              />
            </div>

            {/* AI Semantic Guard (Groq) */}
            <div className="p-3.5 rounded-xl bg-slate-900/30 border border-slate-800/80 space-y-1.5">
              <div className="flex items-center justify-between text-xs">
                <span className="font-medium text-slate-300 flex items-center gap-1.5">
                  <Sparkles className="w-3.5 h-3.5 text-purple-400" />
                  <span>AI Semantic Guard (Groq)</span>
                </span>
                <span className={`text-[10px] ${groqApiKey ? "text-purple-400" : "text-slate-400"}`}>
                  {groqApiKey ? "gpt-oss-20b Online" : "Offline Fallback"}
                </span>
              </div>
              <input
                type="password"
                value={groqApiKey}
                onChange={(e) => handleGroqKeyChange(e.target.value)}
                placeholder="Paste Groq API Key (gsk_...) for LLM matching"
                className="w-full px-2.5 py-1.5 text-xs bg-slate-950 border border-slate-800 rounded text-slate-300 placeholder-slate-600 focus:outline-none focus:border-purple-500"
              />
            </div>

            {/* Session Controller */}
            <div className="p-4 rounded-xl bg-slate-900/60 border border-slate-800 space-y-3">
              <span className="text-xs font-semibold text-slate-400 uppercase tracking-wider block">
                Focus Controls
              </span>

              {!session.is_focus_active ? (
                <div className="grid grid-cols-2 gap-2.5">
                  <button
                    disabled={isStartingSession}
                    onClick={() => handleStartSession(25)}
                    className="flex items-center justify-center gap-2 px-3 py-2.5 bg-emerald-600 hover:bg-emerald-500 active:bg-emerald-700 disabled:opacity-50 disabled:cursor-not-allowed text-white font-medium text-xs rounded-lg transition-colors shadow-sm cursor-pointer"
                  >
                    <Play className="w-3.5 h-3.5" />
                    <span>{isStartingSession ? "Starting..." : "Start 25m Block"}</span>
                  </button>
                  <button
                    disabled={isStartingSession}
                    onClick={() => handleStartSession(50)}
                    className="flex items-center justify-center gap-2 px-3 py-2.5 bg-slate-800 hover:bg-slate-700 active:bg-slate-850 disabled:opacity-50 disabled:cursor-not-allowed text-white font-medium text-xs rounded-lg transition-colors border border-slate-700/80 cursor-pointer"
                  >
                    <Play className="w-3.5 h-3.5" />
                    <span>{isStartingSession ? "Starting..." : "Start 50m Block"}</span>
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
                <span>Native Windows Telemetry</span>
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
        ) : (
          /* Rules Management Tab */
          <div className="space-y-4">
            <form onSubmit={handleAddRule} className="p-3.5 rounded-xl bg-slate-900/60 border border-slate-800 space-y-2.5">
              <span className="text-xs font-semibold text-slate-300 block">Add Process Rule</span>
              <div className="flex gap-2">
                <input
                  type="text"
                  value={newProcessName}
                  onChange={(e) => setNewProcessName(e.target.value)}
                  placeholder="e.g. notion.exe"
                  className="flex-1 px-2.5 py-1.5 text-xs bg-slate-950 border border-slate-700 rounded text-white placeholder-slate-500 focus:outline-none focus:border-emerald-500"
                />
                <select
                  value={newCategory}
                  onChange={(e) => setNewCategory(e.target.value as "whitelist" | "blacklist")}
                  className="px-2.5 py-1.5 text-xs bg-slate-950 border border-slate-700 rounded text-slate-200 focus:outline-none"
                >
                  <option value="whitelist">Whitelist</option>
                  <option value="blacklist">Blacklist</option>
                </select>
                <button
                  type="submit"
                  className="px-3 py-1.5 text-xs bg-emerald-600 hover:bg-emerald-500 text-white rounded font-medium flex items-center gap-1 cursor-pointer"
                >
                  <Plus className="w-3.5 h-3.5" />
                  <span>Add</span>
                </button>
              </div>
            </form>

            <div className="space-y-2 max-h-[300px] overflow-y-auto pr-1">
              {rules.map((rule) => (
                <div
                  key={rule.id}
                  className="flex items-center justify-between p-2.5 rounded-lg bg-slate-900/40 border border-slate-800 text-xs"
                >
                  <div className="flex items-center gap-2">
                    <span
                      className={`w-2 h-2 rounded-full ${
                        rule.category === "whitelist" ? "bg-emerald-400" : "bg-red-400"
                      }`}
                    />
                    <span className="font-mono text-slate-200">{rule.process_name}</span>
                  </div>
                  <div className="flex items-center gap-2">
                    <span
                      className={`text-[10px] px-2 py-0.5 rounded uppercase font-semibold ${
                        rule.category === "whitelist"
                          ? "bg-emerald-500/10 text-emerald-400"
                          : "bg-red-500/10 text-red-400"
                      }`}
                    >
                      {rule.category}
                    </span>
                    <button
                      aria-label={`Delete rule ${rule.process_name}`}
                      onClick={() => handleDeleteRule(rule.id)}
                      className="text-slate-500 hover:text-red-400 transition-colors p-1 cursor-pointer"
                    >
                      <Trash2 className="w-3.5 h-3.5" />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}

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
