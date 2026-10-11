import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { Select } from "../../ui/Select";
import { SettingContainer } from "../../ui/SettingContainer";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { Textarea } from "../../ui/Textarea";
import { ToggleSwitch } from "../../ui/ToggleSwitch";

/**
 * Endpoint configuration for the custom backend upload.
 *
 * This mirrors `ApiConfig` in `crates/handy-voice-api` exactly. It is kept as a
 * hand-written type rather than imported from the generated bindings because the
 * command carries the config as a JSON string (the Rust crate deliberately does
 * not depend on a UI framework), so no type is generated for it.
 */
interface ApiConfig {
  base_url: string;
  endpoint: string;
  method: "POST" | "PUT" | "PATCH";
  headers: Record<string, string>;
  content_type: string;
  timeout_ms: number;
  retry: { max_attempts: number; base_delay_ms: number; max_delay_ms: number };
  idempotency: "enabled" | "disabled";
  request_format:
    | { kind: "transcript_document" }
    | { kind: "wrapped"; field: string }
    | { kind: "template"; template: string };
  allow_insecure_http: boolean;
}

const DEFAULT_CONFIG: ApiConfig = {
  base_url: "",
  endpoint: "transcripts",
  method: "POST",
  headers: {},
  content_type: "application/json",
  timeout_ms: 30000,
  retry: { max_attempts: 3, base_delay_ms: 500, max_delay_ms: 8000 },
  idempotency: "enabled",
  request_format: { kind: "transcript_document" },
  allow_insecure_http: false,
};

/** Parse a `Name: Value` per-line textarea into a header map. */
const parseHeaders = (text: string): Record<string, string> => {
  const out: Record<string, string> = {};
  for (const rawLine of text.split("\n")) {
    const line = rawLine.trim();
    if (!line || line.startsWith("#")) continue;
    const idx = line.indexOf(":");
    if (idx <= 0) continue;
    out[line.slice(0, idx).trim()] = line.slice(idx + 1).trim();
  }
  return out;
};

const formatHeaders = (headers: Record<string, string>): string =>
  Object.entries(headers)
    .map(([k, v]) => `${k}: ${v}`)
    .join("\n");

export const VoiceApiSettings: React.FC = () => {
  const { t } = useTranslation();
  const [config, setConfig] = useState<ApiConfig>(DEFAULT_CONFIG);
  const [headersText, setHeadersText] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [hasKey, setHasKey] = useState(false);
  const [keyStorage, setKeyStorage] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void (async () => {
      const result = await commands.voiceApiGetConfig();
      if (result.status === "ok") {
        const loaded = JSON.parse(result.data) as ApiConfig;
        setConfig(loaded);
        setHeadersText(formatHeaders(loaded.headers ?? {}));
      } else {
        setError(result.error);
      }

      const keyResult = await commands.voiceApiHasApiKey();
      if (keyResult.status === "ok") setHasKey(keyResult.data);
      setKeyStorage(await commands.voiceApiSecretStorage());
    })();
  }, []);

  /**
   * The API key is intentionally absent from this object: it is held in the OS
   * credential store and never round-trips through the settings UI.
   */
  const currentConfig = (): ApiConfig => ({
    ...config,
    headers: parseHeaders(headersText),
  });

  const handleSave = async () => {
    setBusy(true);
    setStatus(null);
    setError(null);
    const result = await commands.voiceApiSetConfig(
      JSON.stringify(currentConfig()),
    );
    if (result.status === "ok") setStatus(t("voiceApi.saved"));
    else setError(result.error);
    setBusy(false);
  };

  const handleSaveKey = async () => {
    if (!apiKey.trim()) return;
    setBusy(true);
    setStatus(null);
    setError(null);
    const result = await commands.voiceApiSetApiKey(apiKey);
    if (result.status === "ok") {
      // Never echo the key back, not even partially.
      setApiKey("");
      setHasKey(true);
      setStatus(t("voiceApi.keySaved"));
    } else {
      setError(result.error);
    }
    setBusy(false);
  };

  const handleClearKey = async () => {
    setBusy(true);
    setStatus(null);
    setError(null);
    const result = await commands.voiceApiClearApiKey();
    if (result.status === "ok") {
      setHasKey(false);
      setStatus(t("voiceApi.keyCleared"));
    } else {
      setError(result.error);
    }
    setBusy(false);
  };

  const handleTest = async () => {
    setBusy(true);
    setStatus(null);
    setError(null);
    const result = await commands.voiceApiTestConnection();
    if (result.status === "ok") {
      const parsed = JSON.parse(result.data) as {
        ok: boolean;
        status: number | null;
        detail: string;
      };
      // The detail string already has any credential redacted by the backend.
      setStatus(`${t("voiceApi.testResult")}: ${parsed.detail}`);
      if (!parsed.ok) setError(parsed.detail);
    } else {
      setError(result.error);
    }
    setBusy(false);
  };

  const format = config.request_format;

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup
        title={t("voiceApi.endpointTitle")}
        description={t("voiceApi.endpointDescription")}
      >
        <SettingContainer
          title={t("voiceApi.baseUrl")}
          description={t("voiceApi.baseUrlDescription")}
          layout="stacked"
        >
          <Input
            value={config.base_url}
            onChange={(e) =>
              setConfig({ ...config, base_url: e.target.value })
            }
            placeholder="https://api.example.com/v1"
          />
        </SettingContainer>

        <SettingContainer
          title={t("voiceApi.endpointPath")}
          description={t("voiceApi.endpointPathDescription")}
          layout="stacked"
        >
          <Input
            value={config.endpoint}
            onChange={(e) => setConfig({ ...config, endpoint: e.target.value })}
            placeholder="transcripts"
          />
        </SettingContainer>

        <SettingContainer
          title={t("voiceApi.method")}
          description={t("voiceApi.methodDescription")}
          layout="stacked"
        >
          <Select
            value={config.method}
            options={["POST", "PUT", "PATCH"].map((m) => ({
              value: m,
              label: m,
            }))}
            onChange={(value) =>
              setConfig({
                ...config,
                method: (value ?? "POST") as ApiConfig["method"],
              })
            }
          />
        </SettingContainer>

        <SettingContainer
          title={t("voiceApi.contentType")}
          description={t("voiceApi.contentTypeDescription")}
          layout="stacked"
        >
          <Input
            value={config.content_type}
            onChange={(e) =>
              setConfig({ ...config, content_type: e.target.value })
            }
          />
        </SettingContainer>
      </SettingsGroup>

      <SettingsGroup
        title={t("voiceApi.bodyTitle")}
        description={t("voiceApi.bodyDescription")}
      >
        <SettingContainer
          title={t("voiceApi.bodyFormat")}
          description={t("voiceApi.bodyFormatDescription")}
          layout="stacked"
        >
          <Select
            value={format.kind}
            options={[
              { value: "transcript_document", label: t("voiceApi.bodyRaw") },
              { value: "wrapped", label: t("voiceApi.bodyWrapped") },
              { value: "template", label: t("voiceApi.bodyTemplate") },
            ]}
            onChange={(value) => {
              const kind = value ?? "transcript_document";
              if (kind === "wrapped") {
                setConfig({
                  ...config,
                  request_format: { kind: "wrapped", field: "transcript" },
                });
              } else if (kind === "template") {
                setConfig({
                  ...config,
                  request_format: {
                    kind: "template",
                    template: '{"transcript":{{transcript}}}',
                  },
                });
              } else {
                setConfig({
                  ...config,
                  request_format: { kind: "transcript_document" },
                });
              }
            }}
          />
        </SettingContainer>

        {format.kind === "wrapped" && (
          <SettingContainer
            title={t("voiceApi.wrapField")}
            description={t("voiceApi.wrapFieldDescription")}
            layout="stacked"
          >
            <Input
              value={format.field}
              onChange={(e) =>
                setConfig({
                  ...config,
                  request_format: { kind: "wrapped", field: e.target.value },
                })
              }
            />
          </SettingContainer>
        )}

        {format.kind === "template" && (
          <SettingContainer
            title={t("voiceApi.template")}
            description={t("voiceApi.templateDescription")}
            layout="stacked"
          >
            <Textarea
              value={format.template}
              onChange={(e) =>
                setConfig({
                  ...config,
                  request_format: {
                    kind: "template",
                    template: e.target.value,
                  },
                })
              }
            />
          </SettingContainer>
        )}
      </SettingsGroup>

      <SettingsGroup
        title={t("voiceApi.authTitle")}
        description={t("voiceApi.authDescription")}
      >
        <SettingContainer
          title={t("voiceApi.apiKey")}
          description={
            hasKey
              ? t("voiceApi.apiKeyStoredIn", { where: keyStorage })
              : t("voiceApi.apiKeyNotSet")
          }
          layout="stacked"
        >
          <div className="flex gap-2 w-full">
            <Input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={hasKey ? "••••••••" : "sk-..."}
              className="flex-1"
            />
            <Button
              variant="primary"
              onClick={handleSaveKey}
              disabled={busy || !apiKey.trim()}
            >
              {t("voiceApi.saveKey")}
            </Button>
            <Button onClick={handleClearKey} disabled={busy || !hasKey}>
              {t("voiceApi.clearKey")}
            </Button>
          </div>
          {/* Deliberately no way to read the key back out of the app. */}
          <p className="text-xs text-mid-gray mt-2">
            {t("voiceApi.apiKeyNote")}
          </p>
        </SettingContainer>

        <SettingContainer
          title={t("voiceApi.customHeaders")}
          description={t("voiceApi.customHeadersDescription")}
          layout="stacked"
        >
          <Textarea
            value={headersText}
            onChange={(e) => setHeadersText(e.target.value)}
            placeholder={"X-Tenant: acme\nX-Trace: abc123"}
          />
        </SettingContainer>
      </SettingsGroup>

      <SettingsGroup
        title={t("voiceApi.behaviourTitle")}
        description={t("voiceApi.behaviourDescription")}
      >
        <SettingContainer
          title={t("voiceApi.timeout")}
          description={t("voiceApi.timeoutDescription")}
          layout="stacked"
        >
          <Input
            type="number"
            value={String(config.timeout_ms)}
            onChange={(e) =>
              setConfig({
                ...config,
                timeout_ms: Number(e.target.value) || 0,
              })
            }
          />
        </SettingContainer>

        <SettingContainer
          title={t("voiceApi.maxAttempts")}
          description={t("voiceApi.maxAttemptsDescription")}
          layout="stacked"
        >
          <Input
            type="number"
            value={String(config.retry.max_attempts)}
            onChange={(e) =>
              setConfig({
                ...config,
                retry: {
                  ...config.retry,
                  max_attempts: Number(e.target.value) || 1,
                },
              })
            }
          />
        </SettingContainer>

        <ToggleSwitch
          checked={config.idempotency === "enabled"}
          onChange={(checked) =>
            setConfig({
              ...config,
              idempotency: checked ? "enabled" : "disabled",
            })
          }
          label={t("voiceApi.idempotency")}
          description={t("voiceApi.idempotencyDescription")}
          grouped
        />

        <ToggleSwitch
          checked={config.allow_insecure_http}
          onChange={(checked) =>
            setConfig({ ...config, allow_insecure_http: checked })
          }
          label={t("voiceApi.allowHttp")}
          description={t("voiceApi.allowHttpDescription")}
          grouped
        />
      </SettingsGroup>

      <div className="flex items-center gap-3 px-4">
        <Button variant="primary" onClick={handleSave} disabled={busy}>
          {t("voiceApi.save")}
        </Button>
        <Button onClick={handleTest} disabled={busy}>
          {busy ? t("voiceApi.working") : t("voiceApi.testConnection")}
        </Button>
      </div>

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
