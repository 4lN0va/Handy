import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";

/**
 * Automatic continuous transcription, and the local verification-tool bridge.
 *
 * Mirrors `ContinuousState`, `ContinuousSettings` and `BridgeConfig` on the Rust
 * side. They cross the boundary as JSON because those structs live in modules
 * that have no reason to depend on a UI framework, so they are declared here
 * rather than generated.
 */

interface ContinuousState {
  running: boolean;
  session_id: string | null;
  segments_completed: number;
  running_ms: number;
  pending_segments: number;
}

interface ContinuousSettings {
  pause_ms: number;
  min_segment_ms: number;
  max_segment_ms: number;
  silence_level: number;
}

interface BridgeConfig {
  enabled: boolean;
  endpoint: string;
  timeout_ms: number;
  heartbeat_secs: number;
  max_attempts: number;
  send_partials: boolean;
}

interface BridgeStatus {
  sent: number;
  failed: number;
  heartbeats: number;
  last_error: string | null;
  last_ok_ms: number | null;
  recording: boolean;
  status: string;
}

const IDLE: ContinuousState = {
  running: false,
  session_id: null,
  segments_completed: 0,
  running_ms: 0,
  pending_segments: 0,
};

function parse<T>(raw: string, fallback: T): T {
  try {
    return JSON.parse(raw) as T;
  } catch {
    return fallback;
  }
}

function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export const ContinuousPanel: React.FC = () => {
  const { t } = useTranslation();
  const [state, setState] = useState<ContinuousState>(IDLE);
  const [settings, setSettings] = useState<ContinuousSettings | null>(null);
  const [bridge, setBridge] = useState<BridgeConfig | null>(null);
  const [bridgeStatus, setBridgeStatus] = useState<BridgeStatus | null>(null);
  const [endpointDraft, setEndpointDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const [s, cfg, bs] = await Promise.all([
      commands.continuousState(),
      commands.bridgeGetConfig(),
      commands.bridgeStatus(),
    ]);
    setState(parse<ContinuousState>(s, IDLE));
    setBridge((prev) => {
      const next = parse<BridgeConfig>(cfg, prev ?? ({} as BridgeConfig));
      setEndpointDraft((d) => (d === "" ? next.endpoint ?? "" : d));
      return next;
    });
    setBridgeStatus(parse<BridgeStatus>(bs, null as unknown as BridgeStatus));
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 1500);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    void (async () => {
      const raw = await commands.continuousGetSettings();
      setSettings(parse<ContinuousSettings | null>(raw, null));
    })();
  }, []);

  const toggle = async () => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const result = state.running
        ? await commands.continuousStop()
        : await commands.continuousStart();
      if (result.status === "error") {
        setError(result.error);
      } else {
        setState(parse<ContinuousState>(result.data, IDLE));
        setNotice(
          state.running
            ? t("transcript.continuous.stoppedNotice")
            : t("transcript.continuous.startedNotice"),
        );
      }
    } finally {
      setBusy(false);
      void refresh();
    }
  };

  const saveBridge = async () => {
    if (!bridge) return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const next: BridgeConfig = { ...bridge, endpoint: endpointDraft.trim() };
      const result = await commands.bridgeSetConfig(JSON.stringify(next));
      if (result.status === "error") {
        setError(result.error);
      } else {
        setBridge(next);
        setNotice(t("transcript.continuous.bridgeSaved"));
        void commands.bridgeHeartbeatNow();
      }
    } finally {
      setBusy(false);
      void refresh();
    }
  };

  const toggleBridge = async () => {
    if (!bridge) return;
    const next: BridgeConfig = { ...bridge, enabled: !bridge.enabled };
    setBusy(true);
    setError(null);
    try {
      const result = await commands.bridgeSetConfig(JSON.stringify(next));
      if (result.status === "error") {
        setError(result.error);
      } else {
        setBridge(next);
        void refresh();
      }
    } finally {
      setBusy(false);
    }
  };

  const updateSetting = async (patch: Partial<ContinuousSettings>) => {
    if (!settings) return;
    const next = { ...settings, ...patch };
    setSettings(next);
    const result = await commands.continuousSetSettings(JSON.stringify(next));
    if (result.status === "error") setError(result.error);
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between gap-4">
        <div>
          <div className="flex items-center gap-2">
            <span
              className={`inline-block h-2 w-2 rounded-full ${
                state.running ? "bg-red-500" : "bg-gray-400"
              }`}
            />
            <span className="font-medium">
              {state.running
                ? t("transcript.continuous.running")
                : t("transcript.continuous.idle")}
            </span>
          </div>
          <p className="mt-1 text-sm opacity-70">
            {t("transcript.continuous.description")}
          </p>
        </div>
        <Button onClick={toggle} disabled={busy}>
          {state.running
            ? t("transcript.continuous.stop")
            : t("transcript.continuous.start")}
        </Button>
      </div>

      {state.running && (
        <div className="flex gap-6 text-sm">
          <span>
            {t("transcript.continuous.elapsed")}: {formatDuration(state.running_ms)}
          </span>
          <span>
            {t("transcript.continuous.segments")}: {state.segments_completed}
          </span>
          {state.pending_segments > 0 && (
            <span>
              {t("transcript.continuous.transcribing")}: {state.pending_segments}
            </span>
          )}
        </div>
      )}

      {settings && (
        <div className="flex flex-wrap items-center gap-4 text-sm">
          <label className="flex items-center gap-2">
            {t("transcript.continuous.pauseMs")}
            <input
              type="number"
              min={200}
              max={10000}
              step={50}
              value={settings.pause_ms}
              onChange={(e) => void updateSetting({ pause_ms: Number(e.target.value) })}
              className="w-24 rounded border border-gray-300 bg-transparent px-2 py-1"
            />
          </label>
          <label className="flex items-center gap-2">
            {t("transcript.continuous.maxSegmentMs")}
            <input
              type="number"
              min={2000}
              max={600000}
              step={1000}
              value={settings.max_segment_ms}
              onChange={(e) => void updateSetting({ max_segment_ms: Number(e.target.value) })}
              className="w-28 rounded border border-gray-300 bg-transparent px-2 py-1"
            />
          </label>
        </div>
      )}

      <div className="border-t border-gray-200 pt-3 dark:border-gray-700">
        <div className="flex items-center justify-between gap-4">
          <div>
            <span className="font-medium">
              {t("transcript.continuous.bridgeTitle")}
            </span>
            <p className="mt-1 text-sm opacity-70">
              {t("transcript.continuous.bridgeDescription")}
            </p>
          </div>
          <Button onClick={toggleBridge} disabled={busy || !bridge}>
            {bridge?.enabled
              ? t("transcript.continuous.bridgeOn")
              : t("transcript.continuous.bridgeOff")}
          </Button>
        </div>

        <div className="mt-3 flex items-center gap-2">
          <input
            type="text"
            value={endpointDraft}
            onChange={(e) => setEndpointDraft(e.target.value)}
            spellCheck={false}
            className="flex-1 rounded border border-gray-300 bg-transparent px-2 py-1 font-mono text-sm"
          />
          <Button onClick={saveBridge} disabled={busy || !bridge}>
            {t("transcript.continuous.save")}
          </Button>
        </div>

        {bridgeStatus && (
          <div className="mt-2 flex flex-wrap gap-4 text-sm opacity-80">
            <span>
              {t("transcript.continuous.sent")}: {bridgeStatus.sent}
            </span>
            <span>
              {t("transcript.continuous.heartbeats")}: {bridgeStatus.heartbeats}
            </span>
            <span
              className={bridgeStatus.failed > 0 ? "text-red-500" : undefined}
            >
              {t("transcript.continuous.failed")}: {bridgeStatus.failed}
            </span>
          </div>
        )}

        {bridgeStatus?.last_error && (
          <p className="mt-1 text-sm text-red-500">{bridgeStatus.last_error}</p>
        )}
      </div>

      {notice && <p className="text-sm text-green-600">{notice}</p>}
      {error && <p className="text-sm text-red-500">{error}</p>}
    </div>
  );
};
