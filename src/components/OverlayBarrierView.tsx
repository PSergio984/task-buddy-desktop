import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ShieldAlert, ArrowRight, ShieldCheck, Flame } from "lucide-react";

const REQUIRED_HOLD_MS = 6000;

interface DiversionPenaltyResult {
  applied: boolean;
  base_damage: number;
  hp_lost: number;
  damage_mitigated: number;
  current_hp: number;
  died: boolean;
}

export const OverlayBarrierView: React.FC = () => {
  const [holdProgress, setHoldProgress] = useState(0); // 0 to 1
  const [isHolding, setIsHolding] = useState(false);
  const [shieldAvailable, setShieldAvailable] = useState(true);
  const [penaltyFeedback, setPenaltyFeedback] = useState<string | null>(null);
  const holdStartTimeRef = useRef<number | null>(null);
  const animFrameRef = useRef<number | null>(null);

  useEffect(() => {
    const checkStatus = () => {
      invoke<{ shield_spent: boolean }>("get_session_status")
        .then((sess) => {
          setShieldAvailable(!sess.shield_spent);
        })
        .catch(console.error);
    };
    checkStatus();

    const unlistenPromise = listen("session-changed", () => {
      checkStatus();
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const isHoldingRef = useRef(false);
  isHoldingRef.current = isHolding;

  useEffect(() => {
    const isControlElement = (el: HTMLElement | null) => {
      if (!el) return false;
      return (
        el.tagName === "BUTTON" ||
        el.tagName === "INPUT" ||
        el.tagName === "TEXTAREA" ||
        el.isContentEditable
      );
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (isControlElement(e.target as HTMLElement | null)) {
        return;
      }
      if (e.code === "Space" && !e.repeat && !isHoldingRef.current) {
        e.preventDefault();
        startHold();
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (isControlElement(e.target as HTMLElement | null)) {
        return;
      }
      if (e.code === "Space" && isHoldingRef.current) {
        e.preventDefault();
        cancelHold();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
    };
  }, []);

  // Cancel any running animation frame only on unmount
  useEffect(() => {
    return () => {
      if (animFrameRef.current) cancelAnimationFrame(animFrameRef.current);
    };
  }, []);

  const startHold = () => {
    setIsHolding(true);
    holdStartTimeRef.current = performance.now();

    const update = (now: number) => {
      if (!holdStartTimeRef.current) return;
      const elapsed = now - holdStartTimeRef.current;
      const progress = Math.min(elapsed / REQUIRED_HOLD_MS, 1);
      setHoldProgress(progress);

      if (progress >= 1) {
        completeBypass();
      } else {
        animFrameRef.current = requestAnimationFrame(update);
      }
    };

    animFrameRef.current = requestAnimationFrame(update);
  };

  const cancelHold = () => {
    setIsHolding(false);
    holdStartTimeRef.current = null;
    if (animFrameRef.current) cancelAnimationFrame(animFrameRef.current);
    setHoldProgress(0);
  };

  const completeBypass = async () => {
    cancelHold();
    try {
      const penalty = await invoke<DiversionPenaltyResult | null>("record_barrier_bypass", {
        reason: "barrier_force_bypass",
      });
      if (penalty && penalty.applied) {
        setPenaltyFeedback(
          `⚠️ Focus Broken: -${penalty.hp_lost} HP (${penalty.damage_mitigated} HP absorbed by gear)`
        );
        setTimeout(async () => {
          await invoke("dismiss_overlay_window");
        }, 1200);
      } else {
        await invoke("dismiss_overlay_window");
      }
    } catch (err) {
      console.error("Failed to record bypass penalty:", err);
      setPenaltyFeedback("Failed to record bypass penalty. Barrier remains locked.");
    }
  };

  const activate5mAllowance = async () => {
    if (!shieldAvailable) return;
    try {
      await invoke("activate_tactical_shield");
      setShieldAvailable(false);
      await invoke("dismiss_overlay_window");
    } catch (err) {
      console.error("Failed to activate tactical shield:", err);
    }
  };

  // SVG Ring Math
  const radius = 64;
  const circumference = 2 * Math.PI * radius;
  const strokeDashoffset = circumference - holdProgress * circumference;

  return (
    <div className="fixed inset-0 z-50 flex flex-col items-center justify-center bg-black/85 backdrop-blur-2xl text-slate-100 select-none p-8">
      {/* Top Warning Badge */}
      <div className="flex items-center gap-2 px-4 py-2 rounded-full bg-amber-500/10 border border-amber-500/20 text-amber-400 text-sm font-medium mb-8 animate-pulse">
        <ShieldAlert className="w-4 h-4" />
        <span>Focus Session Active • Off-Task Diversion Detected</span>
      </div>

      {/* Main Focus Reminder Card */}
      <div className="max-w-md w-full text-center flex flex-col items-center">
        <h1 className="text-3xl font-bold tracking-tight text-white mb-2">
          Pause & Re-Align
        </h1>
        <p className="text-slate-400 text-sm mb-6 leading-relaxed">
          Mindful friction gate engaged. Take a conscious breath before switching contexts or wandering away from your declared work.
        </p>

        {penaltyFeedback && (
          <div className="w-full mb-6 p-3 rounded-lg bg-red-500/20 border border-red-500/40 text-red-300 text-xs flex items-center justify-center gap-2 animate-bounce">
            <Flame className="w-4 h-4 text-red-400 shrink-0" />
            <span>{penaltyFeedback}</span>
          </div>
        )}

        {/* 6-Second Radial Progress Dial */}
        <div
          className="relative flex items-center justify-center cursor-pointer mb-8"
          onMouseDown={startHold}
          onMouseUp={cancelHold}
          onMouseLeave={cancelHold}
          onTouchStart={startHold}
          onTouchEnd={cancelHold}
        >
          <svg className="w-48 h-48 -rotate-90 transform" viewBox="0 0 160 160">
            {/* Background Track */}
            <circle
              cx="80"
              cy="80"
              r={radius}
              className="stroke-slate-800"
              strokeWidth="8"
              fill="transparent"
            />
            {/* Progress Track */}
            <circle
              cx="80"
              cy="80"
              r={radius}
              className="stroke-amber-400 transition-all duration-75 ease-linear"
              strokeWidth="8"
              strokeDasharray={circumference}
              strokeDashoffset={strokeDashoffset}
              strokeLinecap="round"
              fill="transparent"
            />
          </svg>

          {/* Central Button & Hold Percentage */}
          <div className="absolute inset-0 flex flex-col items-center justify-center pointer-events-none">
            <span className="text-3xl font-extrabold font-mono text-white">
              {(6 - holdProgress * 6).toFixed(1)}s
            </span>
            <span className="text-xs uppercase tracking-wider text-slate-400 mt-1 font-semibold">
              {isHolding ? "Keep Holding" : "Hold to Bypass"}
            </span>
          </div>
        </div>

        <p className="text-xs text-slate-500 mb-8">
          Hold <kbd className="px-2 py-1 bg-slate-800 border border-slate-700 rounded text-slate-300 font-mono">Space</kbd> or click and hold the circle for 6 continuous seconds
        </p>

        {/* Secondary Allowance Action */}
        <div className="w-full pt-6 border-t border-slate-800/80 flex items-center justify-between gap-4">
          <div className="text-left">
            <div className="text-xs font-semibold text-slate-300 flex items-center gap-1.5">
              <ShieldCheck className={`w-3.5 h-3.5 ${shieldAvailable ? "text-emerald-400" : "text-slate-500"}`} />
              <span>{shieldAvailable ? "1x Tactical Shield (5m Grace, 0 HP Loss)" : "Tactical Shield Expended"}</span>
            </div>
            <div className="text-[11px] text-slate-500">
              {shieldAvailable
                ? "Emergency 5-minute allowance to reply to a critical message"
                : "Shield already spent for this session"}
            </div>
          </div>
          <button
            onClick={activate5mAllowance}
            disabled={!shieldAvailable}
            className={`px-3 py-1.5 text-xs font-medium rounded-lg transition-colors flex items-center gap-1 ${
              shieldAvailable
                ? "bg-emerald-500/10 hover:bg-emerald-500/20 text-emerald-400 border border-emerald-500/30 cursor-pointer"
                : "bg-slate-800 text-slate-500 border border-slate-700 cursor-not-allowed opacity-60"
            }`}
          >
            <span>{shieldAvailable ? "Use Shield" : "Shield Expended"}</span>
            {shieldAvailable && <ArrowRight className="w-3 h-3" />}
          </button>
        </div>
      </div>
    </div>
  );
};
