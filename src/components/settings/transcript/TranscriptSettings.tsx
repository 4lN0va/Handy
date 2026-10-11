import React, { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { SettingContainer } from "../../ui/SettingContainer";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { Textarea } from "../../ui/Textarea";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { ContinuousPanel } from "./ContinuousPanel";

/**
 * Mirrors `Transcript` in `crates/handy-voice-api`. Carried to the GUI as JSON
 * because the Rust crate does not depend on a UI framework, so it is declared
 * here rather than generated.
 */
interface TranscriptSegment {
  id: number;
  segment_id: string;
  revision: number;
  text: string;
  final: boolean;
  kind: "partial" | "final" | "error";
}

interface TranscriptError {
  message: string;
  at: string;
  segment_id?: string;
}

interface Transcript {
  schemaversion: string;
  sessionid: string;
  created_at: string;
  updated_at: string;
  language: string;
  source: string;
  segments: TranscriptSegment[];
  errors?: TranscriptError[];
}

/** Emitted by the Rust bridge whenever the transcript changes. */
const TRANSCRIPT_UPDATED_EVENT = "voice-api-transcript-updated";

export const TranscriptSettings: React.FC = () => {
  const { t } = useTranslation();
  const [transcript, setTranscript] = useState<Transcript | null>(null);
  const [finalsOnly, setFinalsOnly] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const refresh = useCallback(async () => {
    const result = await commands.voiceApiGetTranscript();
    if (result.status === "ok") {
      setTranscript(JSON.parse(result.data) as Transcript);
    }
    // A missing session is the normal state before the first recording, so a
    // read failure here is not surfaced as an error.
  }, []);

  useEffect(() => {
    void refresh();

    // Live updates pushed from the Rust event bridge.
    const unlisten = listen(TRANSCRIPT_UPDATED_EVENT, () => {
      void refresh();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const handleClear = async () => {
    setError(null);
    const result = await commands.voiceApiClearTranscript();
    if (result.status !== "ok") setError(result.error);
    await refresh();
  };

  const handleCopy = async () => {
    if (!transcript) return;
    setError(null);
    const text = transcript.segments
      .filter((s) => (finalsOnly ? s.final : true))
      .map((s) => s.text)
      .join("\n");
    try {
      await writeText(text);
      setStatus(t("transcript.copied"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const handleExport = async () => {
    setError(null);
    setStatus(null);
    try {
      const path = await save({
        defaultPath: `${transcript?.sessionid ?? "transcript"}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      const result = await commands.voiceApiExportToFile(path, finalsOnly);
      if (result.status === "ok") setStatus(t("transcript.exported", { path }));
      else setError(result.error);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const startEdit = (segment: TranscriptSegment) => {
    setEditing(segment.segment_id);
    setDraft(segment.text);
  };

  const commitEdit = async () => {
    if (!editing) return;
    const result = await commands.voiceApiEditSegment(editing, draft);
    if (result.status === "ok") {
      setEditing(null);
      await refresh();
    } else {
      setError(result.error);
    }
  };

  const segments = transcript?.segments ?? [];
  const committed = segments.filter((s) => s.final);
  const tentative = segments.filter((s) => !s.final);

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("transcript.continuous.title")}>
        <div className="px-4 py-3">
          <ContinuousPanel />
        </div>
      </SettingsGroup>

      <SettingsGroup
        title={t("transcript.title")}
        description={t("transcript.description")}
      >
        <div className="px-4 py-3 space-y-3">
          {segments.length === 0 && (
            <p className="text-sm text-mid-gray">{t("transcript.empty")}</p>
          )}

          {committed.length > 0 && (
            <div>
              <div className="text-xs font-medium text-mid-gray uppercase tracking-wide mb-1">
                {t("transcript.confirmed")}
              </div>
              <ul className="space-y-1">
                {committed.map((s) => (
                  <li key={s.segment_id}>
                    {editing === s.segment_id ? (
                      <div className="space-y-2">
                        <Textarea
                          value={draft}
                          onChange={(e) => setDraft(e.target.value)}
                        />
                        <div className="flex gap-2">
                          <Button variant="primary" onClick={commitEdit}>
                            {t("transcript.save")}
                          </Button>
                          <Button onClick={() => setEditing(null)}>
                            {t("transcript.cancel")}
                          </Button>
                        </div>
                      </div>
                    ) : (
                      <button
                        type="button"
                        onClick={() => startEdit(s)}
                        title={t("transcript.editHint")}
                        className="w-full text-left text-sm rounded px-2 py-1 hover:bg-mid-gray/10"
                      >
                        {s.text}
                      </button>
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {tentative.length > 0 && (
            <div>
              <div className="text-xs font-medium text-mid-gray uppercase tracking-wide mb-1">
                {t("transcript.recognizing")}
              </div>
              <ul className="space-y-1">
                {tentative.map((s) => (
                  <li
                    key={s.segment_id}
                    className="text-sm italic text-mid-gray px-2 py-1"
                  >
                    {s.text}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {transcript?.errors && transcript.errors.length > 0 && (
            <div className="rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2">
              {transcript.errors.map((e, i) => (
                <div key={i} className="text-sm text-red-400">
                  {e.message}
                </div>
              ))}
            </div>
          )}
        </div>
      </SettingsGroup>

      <SettingsGroup title={t("transcript.actionsTitle")}>
        <SettingContainer
          title={t("transcript.finalsOnly")}
          description={t("transcript.finalsOnlyDescription")}
          grouped
        >
          <ToggleSwitch
            checked={finalsOnly}
            onChange={setFinalsOnly}
            label={t("transcript.finalsOnly")}
            description={t("transcript.finalsOnlyDescription")}
            grouped
          />
        </SettingContainer>

        <div className="px-4 py-3 flex flex-wrap gap-2">
          <Button onClick={handleCopy} disabled={segments.length === 0}>
            {t("transcript.copy")}
          </Button>
          <Button onClick={handleExport} disabled={segments.length === 0}>
            {t("transcript.export")}
          </Button>
          <Button
            variant="danger"
            onClick={handleClear}
            disabled={segments.length === 0}
          >
            {t("transcript.clear")}
          </Button>
        </div>
      </SettingsGroup>

      {status && (
        <div className="mx-4 rounded-md border border-mid-gray/20 bg-background px-3 py-2 text-sm">
          {status}
        </div>
      )}
      {error && (
        <div className="mx-4 rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2 text-sm text-red-400">
          {error}
        </div>
      )}
    </div>
  );
};
