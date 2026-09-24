# Live station recognition pilot

Run: 2026-09-24 on the development host. Scope: the first bounded recordings of public stations, authorized by the user on 2026-09-24, recorded and transcribed through the installed CLI and service rather than a research harness. This is qualitative operational evidence. There are no reference transcripts, so no error rate is claimed, and no language is qualified.

## Method

One Radio Browser page of healthy `news` stations was refreshed for each survey language (Spanish, Arabic, Hindi, French, Portuguese, Swahili, Chinese; 25 stations per page, 120 cached). Eight stations were registered with `radio add --redirects public`. Direct streams were recorded with `record start` for 60 and then 40 seconds, `--max-mib 8`, two at a time. Retained recordings were pinned, published and transcribed with `analysis transcribe` using the `turbo-q5-cpu` profile: whisper.cpp b5130, `ggml-large-v3-turbo-q5_0`, the Silero VAD, four threads, a 3 GiB committed-memory ceiling. The service binary was a debug build. Recordings stayed in a local library and are not published. No click or vote was sent.

## Acquisition results

| Station | Language hint | Transport | Result |
| --- | --- | --- | --- |
| Radio Exterior de Espana | Spanish | Direct MP3 | Recorded |
| MC Doualiya | Arabic | Direct MP3 | Recorded |
| RFI Brasil | Portuguese | Direct MP3 | Recorded |
| RFI Kiswahili | Swahili | Direct MP3 | Recorded |
| ICI Premiere Montreal | French (Canada) | Direct MP3 behind one redirect | Failed at first (`connection or response headers`); recorded after the timeout fix |
| CRI Chinese service | Chinese | Live HLS | Failed: `unsupported audio content type` |
| Al Jazeera Arabic | Arabic | Live HLS | Not attempted: live HLS |
| AIR Delhi FM Gold | Hindi | Live HLS | Not attempted: live HLS |

A 60-second request published 63 to 74 seconds of decoded audio, because stations deliver a buffered burst at connect. That exceeded the recognizer's 60-second interval limit, so the four stations were recorded again for 40 seconds (43 to 55 seconds published).

**ICI Premiere root cause.** The stream edge that the redirect points to waits about 5.4 seconds before sending HTTP/1.0 headers, measured five times with different request headers. The acquirer's per-read timeout was 5 seconds, so it abandoned the response just before it began. The timeout is now 12 seconds, with a regression test that serves headers after 6.5 seconds; that test fails with the original value and passes with the new one. After the fix, a 40-second recording of the station published 44.4 seconds and transcribed as described below.

**Live HLS.** Three of the eight stations, covering Chinese, Hindi and one Arabic broadcaster, publish only live HLS. `record hls` accepts only finite media playlists, and a master playlist fails closed. Live HLS recording is a material coverage gap for world radio.

## Recognition results

| Recording | Published audio | Wall time | Cues | Language evidence | Observation |
| --- | ---: | ---: | ---: | --- | --- |
| Spanish | 43.1 s | 151 s | 3 | `es` | Fluent, accurate interview text about Iberian ham; punctuation and capitalization present |
| Arabic | 45.6 s | 274 s | 16+ | `ar` | A Tunisian-dialect interview (for example the dialect word برشا) rendered as readable dialectal Arabic |
| Portuguese | 54.7 s | 247 s | 11 | `pt` | Fluent, accurate report on papal visits |
| Canadian French | 44.4 s | 148 s (release build) | 16+ | `fr` | Accurate Quebec French, including regional forms such as `ces belugas-la`; the block label is `fr`, because the recognizer does not distinguish regional varieties |
| Swahili | 48.4 s | 213 s | 2 | `sw` | Kenyan political report, understandable but with spelling errors (`raisu` for rais, a garbled name for William Ruto), no punctuation, and natural Swahili-English code switching (`statesman`, `rebel leader`) |

Wall time includes decoding, profile hashing in a debug build, and CPU contention with a concurrent calibration run on the same host. It is not a capacity measurement. Every job reported zero USD and ended with its process group empty.

## Translation follow-up

The same day, the Canadian French transcript was translated through `analysis translate` with llama.cpp b11146 and Hy-MT2 1.8B Q4_K_M (four threads, one contained process per cue). The first run, on a host saturated by parallel builds, finished 7 of 20 cues before their 120-second deadlines. After contained workers switched to passive OpenMP waiting, a second run translated all 20 cues in 76 seconds. The Swahili transcript stayed untranslated as an undeclared language.

Observed errors, without a reference translation: `itinerance` became "itinerancy", although in Quebec French it means homelessness, a regional false friend; `Plonge` became "Trapped" instead of "Immersed"; and a cue that continues a sentence (`soit la cause`) became "or the cause", because each cue is translated without its neighbors. Cue-level translation therefore needs bounded neighboring context, and Canadian French needs its own reference checks.

## Live HLS follow-up

After [live HLS](../../../docs/decisions/0042-live-hls.md) landed, the three HLS stations were tried the same day. CRI Chinese served a live media playlist directly; `record hls --live` published 45.1 seconds of MPEG transport stream. Al Jazeera Arabic served a master playlist with one audio-only AAC variant; after `source playlist resolve` and an explicit accept, it recorded 60.0 seconds. Both transcribed through the service: CRI as a serialized historical novel with homophone errors on classical court terms (for example 预纸 for 谕旨, an imperial edict), and Al Jazeera as coherent Modern Standard Arabic discussion. AIR Delhi's master playlist repeats `CODECS` in every variant and was rejected; the parser now treats a repeated `CODECS` as unknown. Its segments are ADTS AAC served as `audio/x-aac`, which is now accepted as an AAC alias. After both fixes, AIR Delhi recorded 58.5 seconds of live HLS. It was playing a Hindi film song: the recognizer produced one lyric line labeled `hi`, and the speech-activity gate excluded the rest as non-speech. With this, all eight pilot stations have been recorded and transcribed through the service.

## Findings

- The end-to-end path works on live broadcasts: directory, registration, recording, pinning, contained recognition, original-script storage and language evidence.
- A block language label is correct for these four recordings, but the Swahili recording mixes English, which a block label cannot represent.
- Station audio over 60 seconds needs chunked recognition before routine use.
- Live HLS support and the stream-edge timeout were the two acquisition gaps found; the timeout is fixed.

## Limitations

Four recordings, one run each, no references, one host, a debug service build and a shared CPU. Directory language tags are hints. This is the first Canadian French recognition on real broadcast audio. It is qualitative, and it does not qualify Canadian French, which needs a reference-scored set.
