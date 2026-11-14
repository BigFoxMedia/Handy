import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { commands, type HealthSnapshot } from "../../bindings";
import { ModelStateEvent } from "@/lib/types/events";
import { useSettings } from "../../hooks/useSettings";

type LocalStatus = "loaded" | "unloaded";
type LastSource = "remote" | "local-fallback" | "local" | null;

const Pill: React.FC<{ healthy: boolean; label: string; title: string }> = ({
  healthy,
  label,
  title,
}) => (
  <span
    title={title}
    className={`inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs border ${
      healthy
        ? "border-green-500 text-green-700 dark:text-green-400"
        : "border-mid-gray/40 text-text/40"
    }`}
  >
    <span
      className={`inline-block h-1.5 w-1.5 rounded-full ${
        healthy ? "bg-green-500" : "bg-mid-gray/60"
      }`}
    />
    {label}
  </span>
);

export const RemoteStatusBadge: React.FC = () => {
  const { t } = useTranslation();
  const { settings } = useSettings();
  const [snapshot, setSnapshot] = useState<HealthSnapshot | null>(null);
  const [localStatus, setLocalStatus] = useState<LocalStatus>("unloaded");
  const [lastSource, setLastSource] = useState<LastSource>(null);

  const enabled = settings?.remote_transcription_enabled ?? false;

  useEffect(() => {
    if (!enabled) {
      setSnapshot(null);
      return;
    }
    let cancelled = false;
    let cancel: UnlistenFn | null = null;
    (async () => {
      const res = await commands.getRemoteTranscriptionHealth();
      if (!cancelled && res.status === "ok") setSnapshot(res.data);
      const unlisten = await listen<HealthSnapshot>(
        "remote-stt-health-changed",
        (e) => setSnapshot(e.payload),
      );
      if (cancelled) {
        unlisten();
      } else {
        cancel = unlisten;
      }
    })();
    return () => {
      cancelled = true;
      if (cancel) cancel();
    };
  }, [enabled]);

  useEffect(() => {
    let cancelled = false;
    let cancelLocal: UnlistenFn | null = null;
    let cancelSrc: UnlistenFn | null = null;
    (async () => {
      const unlistenLocal = await listen<ModelStateEvent>(
        "model-state-changed",
        (e) => {
          if (e.payload.event_type === "loading_completed") {
            setLocalStatus("loaded");
          } else if (e.payload.event_type === "unloaded") {
            setLocalStatus("unloaded");
          }
        },
      );
      if (cancelled) unlistenLocal();
      else cancelLocal = unlistenLocal;

      const unlistenSrc = await listen<{ source: LastSource }>(
        "transcription-source",
        (e) => setLastSource(e.payload.source),
      );
      if (cancelled) unlistenSrc();
      else cancelSrc = unlistenSrc;
    })();

    // Read the current state once, so a model loaded before this component
    // mounted is not shown as unloaded.
    commands.getModelLoadStatus().then((res) => {
      if (!cancelled && res.status === "ok") {
        setLocalStatus(res.data.is_loaded ? "loaded" : "unloaded");
      }
    });

    return () => {
      cancelled = true;
      if (cancelLocal) cancelLocal();
      if (cancelSrc) cancelSrc();
    };
  }, []);

  if (!enabled) return null;

  const remoteHealthy = snapshot?.healthy ?? false;
  const remoteTitle = snapshot
    ? `${snapshot.provider_label || snapshot.provider_id}${
        snapshot.last_error ? ` — ${snapshot.last_error}` : ""
      }`
    : t("remoteTranscription.badge.unknown");

  const localTitle =
    localStatus === "loaded"
      ? t("remoteTranscription.badge.localLoaded")
      : t("remoteTranscription.badge.localUnloaded");

  return (
    <div className="flex items-center gap-1.5">
      <Pill
        healthy={remoteHealthy}
        label={t("remoteTranscription.badge.remote")}
        title={remoteTitle}
      />
      <Pill
        healthy={localStatus === "loaded"}
        label={t("remoteTranscription.badge.local")}
        title={localTitle}
      />
      {lastSource && (
        <span className="text-xs text-text/40">
          {t(`remoteTranscription.badge.source.${lastSource}`)}
        </span>
      )}
    </div>
  );
};
