import { useEffect, useState, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ShieldAlert, Clock, ArrowRight } from "lucide-react";

const REQUIRED_HOLD_MS = 6000;

export const OverlayBarrierView: React.FC = () => {
  const [holdProgress, setHoldProgress] = useState(0); // 0 to 1
  const [isHolding, setIsHolding] = useState(false);
  const holdStartTimeRef = useRef<number | null>(null);
  const animFrameRef = useRef<number | null>(null);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code === "Space" && !e.repeat && !isHolding) {
        e.preventDefault();
        startHold();
      }
    };

    const handleKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") {
        e.preventDefault();
        cancelHold();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("keyup", handleKeyUp);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("keyup", handleKeyUp);
      if (animFrameRef.current) cancelAnimationFrame(animFrameRef.current);
    };
  }, [isHolding]);

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
      await invoke("dismiss_barrier");
    } catch (err) {
      console.error("Failed to dismiss barrier:", err);
    }
  };

  const activate5mAllowance = async () => {
    try {
      await invoke("dismiss_barrier");
    } catch (err) {
      console.error("Failed to dismiss barrier for allowance:", err);
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
        <p className="text-slate-400 text-sm mb-10 leading-relaxed">
          Mindful friction gate engaged. Take a conscious breath before switching contexts or wandering away from your declared work.
        </p>

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
              <Clock className="w-3.5 h-3.5 text-blue-400" />
              <span>5-Minute Quick Allowance</span>
            </div>
            <div className="text-[11px] text-slate-500">
              Need to reply to a coworker on Messenger?
            </div>
          </div>
          <button
            onClick={activate5mAllowance}
            className="px-3 py-1.5 text-xs font-medium bg-blue-500/10 hover:bg-blue-500/20 text-blue-400 border border-blue-500/30 rounded-lg transition-colors flex items-center gap-1"
          >
            <span>Activate 5m</span>
            <ArrowRight className="w-3 h-3" />
          </button>
        </div>
      </div>
    </div>
  );
};
