import React, { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { RefreshCcw } from "lucide-react";
import { SettingContainer, SettingsGroup } from "@/components/ui";
import { Button } from "../../ui/Button";
import { ResetButton } from "../../ui/ResetButton";
import { useSettings } from "../../../hooks/useSettings";
import { ProviderSelect } from "../PostProcessingSettingsApi/ProviderSelect";
import { BaseUrlField } from "../PostProcessingSettingsApi/BaseUrlField";
import { ApiKeyField } from "../PostProcessingSettingsApi/ApiKeyField";
import { ModelSelect } from "../PostProcessingSettingsApi/ModelSelect";
import { RemoteTranscriptionEnableToggle } from "./EnableToggle";
import type { ModelOption } from "../PostProcessingSettingsApi/types";

export const RemoteTranscriptionSettings: React.FC = () => {
  const { t } = useTranslation();
  const {
    settings,
    isUpdating,
    remoteTranscriptionModelOptions,
    setRemoteTranscriptionProvider,
    updateRemoteTranscriptionBaseUrl,
    updateRemoteTranscriptionApiKey,
    updateRemoteTranscriptionHealthEndpoint,
    updateRemoteTranscriptionModel,
    fetchRemoteTranscriptionModels,
    testRemoteTranscriptionConnection,
  } = useSettings();

  const [testStatus, setTestStatus] = useState<string | null>(null);

  const enabled = settings?.remote_transcription_enabled ?? false;
  const providers = settings?.remote_transcription_providers ?? [];
  const providerId =
    settings?.remote_transcription_provider_id ?? providers[0]?.id ?? "custom";
  const provider = providers.find((p) => p.id === providerId);
  const baseUrl = provider?.base_url ?? "";
  const healthEndpoint = provider?.health_endpoint ?? "";
  const apiKey = settings?.remote_transcription_api_keys?.[providerId] ?? "";
  const model = settings?.remote_transcription_models?.[providerId] ?? "";

  useEffect(() => {
    setTestStatus(null);
  }, [providerId]);

  const providerOptions = useMemo(
    () => providers.map((p) => ({ value: p.id, label: p.label })),
    [providers],
  );

  const availableModelsRaw = remoteTranscriptionModelOptions[providerId] ?? [];

  const modelOptions = useMemo<ModelOption[]>(() => {
    const seen = new Set<string>();
    const opts: ModelOption[] = [];
    const push = (v: string | null | undefined) => {
      const trimmed = v?.trim();
      if (!trimmed || seen.has(trimmed)) return;
      seen.add(trimmed);
      opts.push({ value: trimmed, label: trimmed });
    };
    for (const m of availableModelsRaw) push(m);
    push(model);
    return opts;
  }, [availableModelsRaw, model]);

  const isFetchingModels = isUpdating(
    `remote_transcription_models_fetch:${providerId}`,
  );
  const isModelUpdating = isUpdating(
    `remote_transcription_model:${providerId}`,
  );
  const isBaseUrlUpdating = isUpdating(
    `remote_transcription_base_url:${providerId}`,
  );
  const isApiKeyUpdating = isUpdating(
    `remote_transcription_api_key:${providerId}`,
  );
  const isHealthEndpointUpdating = isUpdating(
    `remote_transcription_health_endpoint:${providerId}`,
  );

  const allowBaseUrlEdit = provider?.allow_base_url_edit ?? false;

  const handleTest = async () => {
    setTestStatus(t("remoteTranscription.test.inProgress"));
    const res = await testRemoteTranscriptionConnection(providerId);
    if (res.ok) {
      setTestStatus(t("remoteTranscription.test.success"));
    } else {
      setTestStatus(
        t("remoteTranscription.test.failure", { error: res.error }),
      );
    }
  };

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("remoteTranscription.enable.title")}>
        <RemoteTranscriptionEnableToggle />

        {enabled && (
          <>
            <SettingContainer
              title={t("remoteTranscription.provider")}
              description={t(
                "settings.postProcessing.api.provider.description",
              )}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <ProviderSelect
                  options={providerOptions}
                  value={providerId}
                  onChange={setRemoteTranscriptionProvider}
                />
              </div>
            </SettingContainer>

            {allowBaseUrlEdit && (
              <SettingContainer
                title={t("settings.postProcessing.api.baseUrl.title")}
                description={t(
                  "settings.postProcessing.api.baseUrl.description",
                )}
                descriptionMode="tooltip"
                layout="horizontal"
                grouped={true}
              >
                <div className="flex items-center gap-2">
                  <BaseUrlField
                    value={baseUrl}
                    onBlur={(v) =>
                      updateRemoteTranscriptionBaseUrl(providerId, v.trim())
                    }
                    disabled={isBaseUrlUpdating}
                    className="min-w-[380px]"
                  />
                </div>
              </SettingContainer>
            )}

            {allowBaseUrlEdit && (
              <SettingContainer
                title={t("remoteTranscription.healthEndpoint.title")}
                description={t(
                  "remoteTranscription.healthEndpoint.description",
                )}
                descriptionMode="tooltip"
                layout="horizontal"
                grouped={true}
              >
                <div className="flex items-center gap-2">
                  <BaseUrlField
                    value={healthEndpoint}
                    onBlur={(v) =>
                      updateRemoteTranscriptionHealthEndpoint(providerId, v)
                    }
                    disabled={isHealthEndpointUpdating}
                    placeholder="/models"
                    className="min-w-[380px]"
                  />
                </div>
              </SettingContainer>
            )}

            <SettingContainer
              title={t("settings.postProcessing.api.apiKey.title")}
              description={t("settings.postProcessing.api.apiKey.description")}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <ApiKeyField
                  value={apiKey}
                  onBlur={(v) =>
                    updateRemoteTranscriptionApiKey(providerId, v.trim())
                  }
                  disabled={isApiKeyUpdating}
                  className="min-w-[320px]"
                />
              </div>
            </SettingContainer>

            <SettingContainer
              title={t("remoteTranscription.model")}
              description={t(
                "settings.postProcessing.api.model.descriptionDefault",
              )}
              descriptionMode="tooltip"
              layout="stacked"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <ModelSelect
                  value={model}
                  options={modelOptions}
                  disabled={isModelUpdating}
                  isLoading={isFetchingModels}
                  placeholder={
                    modelOptions.length > 0
                      ? t(
                          "settings.postProcessing.api.model.placeholderWithOptions",
                        )
                      : t(
                          "settings.postProcessing.api.model.placeholderNoOptions",
                        )
                  }
                  onSelect={(v) =>
                    updateRemoteTranscriptionModel(providerId, v.trim())
                  }
                  onCreate={(v) =>
                    updateRemoteTranscriptionModel(providerId, v)
                  }
                  onBlur={() => {}}
                  className="flex-1 min-w-[380px]"
                />
                <ResetButton
                  onClick={() => fetchRemoteTranscriptionModels(providerId)}
                  disabled={isFetchingModels}
                  ariaLabel={t("remoteTranscription.fetchModels.label")}
                  className="flex h-10 w-10 items-center justify-center"
                >
                  <RefreshCcw
                    className={`h-4 w-4 ${isFetchingModels ? "animate-spin" : ""}`}
                  />
                </ResetButton>
              </div>
            </SettingContainer>

            <SettingContainer
              title={t("remoteTranscription.test.button")}
              description={t("remoteTranscription.test.description")}
              descriptionMode="tooltip"
              layout="horizontal"
              grouped={true}
            >
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  onClick={handleTest}
                  variant="secondary"
                  size="md"
                >
                  {t("remoteTranscription.test.button")}
                </Button>
                {testStatus && <span className="text-sm">{testStatus}</span>}
              </div>
            </SettingContainer>
          </>
        )}
      </SettingsGroup>
    </div>
  );
};
