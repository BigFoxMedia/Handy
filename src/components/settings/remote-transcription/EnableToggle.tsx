import React from "react";
import { useTranslation } from "react-i18next";
import { ToggleSwitch } from "../../ui/ToggleSwitch";
import { useSettings } from "../../../hooks/useSettings";

export const RemoteTranscriptionEnableToggle: React.FC = React.memo(() => {
  const { t } = useTranslation();
  const { getSetting, updateSetting, isUpdating } = useSettings();

  const enabled = getSetting("remote_transcription_enabled") ?? false;

  return (
    <ToggleSwitch
      checked={enabled}
      onChange={(next) => updateSetting("remote_transcription_enabled", next)}
      isUpdating={isUpdating("remote_transcription_enabled")}
      label={t("remoteTranscription.enable.title")}
      description={t("remoteTranscription.enable.description")}
      grouped={true}
    />
  );
});

RemoteTranscriptionEnableToggle.displayName = "RemoteTranscriptionEnableToggle";
