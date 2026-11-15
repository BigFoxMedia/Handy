# Remote speech-to-text

Handy can send recordings to an OpenAI-compatible speech-to-text server
instead of running the local model. It is opt-in: with remote transcription
disabled, behavior is unchanged. By default the full recording is sent once
at stop; an optional Live preview sends chunks while recording and displays
the returned text in the Live overlay.

## Settings

Settings → Remote STT:

- **Remote transcription** — enable the feature.
- **Live preview** — send chunks during recording and show live text. Requires
  the Live overlay style. Preview audio is uploaded while recording; the
  final text still comes from the full recording. Default off.
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

Live preview uses the same endpoint with bounded, growing windows: it sends
the current speech window as a WAV every ~1.5 s of new audio, and commits text
at a detected pause or a 12 s ceiling. Preview text is approximate; the final
full-recording batch request is authoritative. A preview failure stops the
live preview only, it does not trigger the local fallback.

## Fallback

A failed final remote request — configuration, pre-flight probe, network, HTTP
status, timeout, or malformed response — falls back to the local engine. If
remote transcription was enabled without a local model selected, the local
model is loaded on demand for the fallback. A failed fallback is reported
through the app's notice system.

Preview uploads count toward the endpoint's request/quota usage: enabling Live
preview sends audio during recording, not only at stop.

The remote health monitor polls the configured endpoint every 30 seconds and
drives the footer Remote/Local status badge. A green Remote badge means the
health probe succeeds; a failed probe does not prevent the transcription
request from being attempted.

## Scope

The remote path currently covers live dictation and the headless
`--transcribe-file` path. Background history re-transcription and file jobs
still use the local engine. Live preview applies only to dictation with the
Live overlay style.

## LiteLLM endpoint

The whisper slot is exposed through the fleet-facing LiteLLM as the model
`stt-whisper-local`. Handy can use:

```text
Base URL: https://litellm.apps-prod.enpraise.com/v1
Model:    stt-whisper-local
API key:  your LiteLLM key
```

This avoids putting the LAN IP into Handy. LiteLLM internally points the model
at the whisper endpoint; Live preview works because the LiteLLM model forwards
the same OpenAI-compatible `/v1/audio/transcriptions` requests.

## Build artifacts

Linux (amd64 `.deb`) from the dev container:

```bash
./scripts/dev bun run tauri build --bundles deb \
  --config src-tauri/tauri.fork.conf.json
```

Current 0.10.1 build:

```text
Handy_0.10.1_amd64.deb
sha256 7cd66476944ba53ef17644d6c7f9449bc7cc269ec21446090a3f8898c21e6911
```

Windows installers are built by `.github/workflows/fork-build.yml` on
`windows-latest`; it uses the same fork config to disable updater artifacts and
make the Windows signing command a no-op. Upstream's signed/Azure build is
unchanged.

The 0.10.1 fork build produced both installer formats (unsigned):

```text
Handy_0.10.1_x64-setup.exe   # NSIS
sha256 1e5952c9b847a74d7e2a884a7b888c43ce00c7a0b9fcf26755a59bcbfa445eb9

Handy_0.10.1_x64_en-US.msi   # MSI
sha256 692a9add44e7a3a6d99213e5cb859d2573af9bd1c3040bfbf4b6edf5a4ff123a
```

Because they are unsigned, Windows SmartScreen may warn on first launch.
