# Remote STT manual QA — 2026-11-14

Scope: the Remote Transcription settings section, the remote health badge,
the remote transcription path, the Live preview toggle, and the local fallback
path.

Environment: Handy 0.10.1 built from `feat/remote-stt-v2`, run headlessly under
Xvfb in the dev container. The endpoint in the screenshots is a local mock at
`http://127.0.0.1:9100/v1`, not a production or private endpoint.

## Flows

| Flow                                                                                                                                                | Evidence                           | Verdict                                                                                                                     |
| --------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Remote STT section is visible but disabled; no remote fields until enabled                                                                          | `01-remote-disabled.png`           | PASS                                                                                                                        |
| Enabling remote reveals provider/base URL/health path/API key/model/test controls; default provider is an empty Custom (OpenAI-compatible) endpoint | `02-remote-enabled-empty.png`      | PASS                                                                                                                        |
| Filling base URL + model and testing the connection reports `Connection OK`; the footer Remote badge turns green                                    | `03-remote-configured-test-ok.png` | PASS                                                                                                                        |
| Live preview toggle is present and can be enabled when the Live overlay style is active                                                              | `05-live-preview-toggle.png`       | PASS (UI)                                                                                                                   |
| `handy --transcribe-file` uses the remote path when remote is enabled (mock endpoint)                                                               | `04-cli-evidence.txt`              | PASS                                                                                                                        |
| Handy reaches the whisper model through LiteLLM (`stt-whisper-local`), without putting the LAN IP in Handy                                          | `06-litellm-cli-evidence.txt`      | PASS                                                                                                                        |
| A failing remote endpoint triggers the local fallback path                                                                                          | `04-cli-evidence.txt`              | PASS (fallback attempted; QA container has no local model, so local transcription then reports the expected no-model error) |

## Notes

- The remote endpoint contract is OpenAI-compatible:
  `POST {base_url}/audio/transcriptions` (or `/audio/translations` when
  translating) with multipart WAV and optional bearer auth.
- Health checks default to `{base_url}/models`. The optional “Health check path”
  field lets a server expose a different probe path.
- Background history re-transcription and file jobs are not covered by the
  remote path in this QA run; they continue to use the local engine.
- Live preview text rendering needs a real microphone. The Xvfb QA container
  has no microphone, so the UI toggle and preview client/chunking tests are
  the evidence here; the on-device live-text behavior is exercised on the
  Windows laptop after this build.
- Screenshots were captured at 1280x800 under Xvfb; the app window is 720x600.
