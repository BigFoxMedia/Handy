# Remote speech-to-text

Handy can send a finished recording to an OpenAI-compatible speech-to-text
server instead of running the local model. It is opt-in: with remote
transcription disabled, behavior is unchanged.

## Settings

Settings → Remote STT:

- **Remote transcription** — enable the feature.
- **Provider** — OpenAI, Groq, Speaches, faster-whisper-server, LocalAI, or
  Custom (OpenAI-compatible).
- **Base URL** — editable only for the local/custom providers. Enter the
  endpoint including its version path, for example `http://server:9100/v1`.
- **Health check path** — optional path appended to the base URL for the
  pre-flight probe. Blank means `/models`.
- **API key** — optional. Empty means no `Authorization` header.
- **Model** — the server's model name. The refresh button fetches `/models`
  from the endpoint.
- **Test connection** — probes the health path.

The default Custom and LocalAI providers are intentionally empty: the user
fills in the base URL and model for their own server. No endpoint address is
hardcoded.

## Request contract

Remote transcription sends 16 kHz mono PCM as a WAV upload:

```text
POST {base_url}/audio/transcriptions
Content-Type: multipart/form-data
Authorization: Bearer <key>      # omitted when the key is empty

fields: file=audio.wav, model=<model>, response_format=json,
        language=<code>          # omitted for auto
        prompt=<custom words>    # omitted when empty
```

With translation enabled it posts to `/audio/translations` instead.

## Fallback

Any remote failure — configuration, pre-flight probe, network, HTTP status,
timeout, or malformed response — falls back to the local engine. If remote
transcription was enabled without a local model selected, the local model is
loaded on demand for the fallback. A failed fallback is reported through the
app's notice system.

The remote health monitor polls the configured endpoint every 30 seconds and
drives the footer Remote/Local status badge. A green Remote badge means the
health probe succeeds; a failed probe does not prevent the transcription
request from being attempted.

## Scope

The remote path currently covers live dictation and the headless
`--transcribe-file` path. Background history re-transcription and file jobs
still use the local engine.

## Build artifacts

Linux (amd64 `.deb`) from the dev container:

```bash
./scripts/dev bun run tauri build --bundles deb \
  --config src-tauri/tauri.fork.conf.json
```

Windows (unsigned NSIS installer) is built by
`.github/workflows/fork-build.yml` on `windows-latest`; it uses the same fork
config to disable updater artifacts and make the Windows signing command a
no-op. Upstream's signed/Azure build is unchanged.
