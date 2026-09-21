# Recording envelopes and signal-specific metadata

Updated: 2026-09-21. The HTTP audio export below is implemented. Other profiles are planned contracts, not supported adapters.

## Common envelope

`record metadata ID` emits UTF-8 JSON with `schema: "sigy.recording"` and `schema_version: 1`. The serializer types in `sigy-service::recordings::metadata` are the current export contract. There is no sidecar importer. Exported metadata does not grant permissions or authorize processing.

| Section | Current fields and meaning |
| --- | --- |
| Identity | Recording ID and independently versioned metadata schema |
| Source | Immutable revision, acquisition adapter, original name, redacted origin, network scope |
| Capture | Journal state, revision/generation, explicitly labeled planned UTC window, byte ceiling, end reason and bounded failure detail |
| Payload | Tagged `encoded_audio` variant, decoder format and decoded duration when validation succeeds |
| Storage | Retention at export, retained byte count, SHA-256, storage state, explicit processing acknowledgment |

An exported sidecar is a snapshot. The catalog remains authoritative for later retention changes and deletion. Deleted history preserves earlier byte/hash evidence but reports that media is no longer retained. A planned window is not a measured reception interval, and decoded duration is not a claim about station transmission time. Missing observations remain absent. Current exports do not claim a language, transmitter location, RF frequency, topic, song, or completed automatic analysis.

## Extension contract

Extend acquisition, payload and interpretation separately. A source family is not a codec, a radio protocol is not necessarily audio, and an interpretation never replaces the original observation. The next envelope revision must add typed artifact IDs, parent/input references, adapter and configuration versions, actual clock mappings, gaps, completeness and processing revisions before these families dispatch.

| Source/profile | Payload and additional evidence |
| --- | --- |
| Internet radio | Encoded audio, directory identities and refresh provenance, station versus transport metadata, captured timeline and gaps |
| Podcast episode | Feed identity, publisher episode GUID, enclosure revision, publication time distinct from acquisition time, supplied transcript/chapters with provenance |
| SDR/receiver | Real or IQ samples, numeric representation, byte order, sample rate, channels, tuning epochs, center frequency, acquisition bandwidth, gain and receiver/antenna profile |
| CB or another band preset | Region/profile revision, channel-to-frequency mapping, modulation and receiver capabilities; the preset does not create hardware support or operating permission |
| LoRa reception | Raw observations or frames, frequency/bandwidth, spreading factor, coding rate, integrity checks, RSSI/SNR units and receiver timestamps when available |
| Meshtastic node | Versioned protocol packets, node/interface identity, message/telemetry type, supplied-key/authentication status, clock uncertainty and links to underlying observations where available |
| Spectrum/measurement | Time/frequency intervals, FFT/window/bin definitions, averaging, units and calibration basis; uncalibrated power is not labeled as calibrated RF strength |
| Derived audio, text, symbols, telemetry or images | Typed parent references, transform identity, alignment, revisions, confidence/abstention and propagated gaps |

Radio captures may contain multiple channels or a changing tuning configuration. A single free-text frequency field is insufficient. Use explicit hertz units, validated intervals, and sample indices with clock mappings. LoRa frames and Meshtastic messages are separate layers. Internet station geography is not receiver position or transmitter evidence. Unknown metadata must not be synthesized from an LLM guess.

For RF sample interchange, implement and validate a SigMF projection rather than inventing an incompatible sample-file standard. Keep Sigy's application envelope for lifecycle, policies, source ancestry and derived artifacts. Do not claim SigMF compliance until both the chosen specification schema and sample layout pass interoperability tests. [Research](../../research/26-recording-discovery-and-rf.md).

## Receiver exploration roadmap

Expose device capabilities before enabling controls: tunable ranges, instantaneous bandwidth, sample formats/rates, supported demodulators, gain controls and exclusivity. Validate requested profiles against device capabilities. One tuner cannot independently receive arbitrary simultaneous bands; channelization is possible only within the actually sampled span and measured processing budget.

Plan receive-only band browsing, region-aware presets including CB, spectrum/waterfall views, frequency bookmarks, bounded sweeps, occupancy measurements, candidate signal detection, and scheduled capture. Record dwell times and unobserved intervals during scans. A detected peak is not an identified transmitter, and a scan is not complete simultaneous coverage. Preserve raw observations where retention permits so later demodulators and decoders can revisit them.

Internet radio and podcasts precede physical radio integration. Hardware qualification uses the user's actual receiver and LoRa device when ready, with driver, antenna, overload, calibration, disconnect and resource tests. Band plans and legal constraints require jurisdiction-specific current research before presets ship.

### Receiver modes and playful inspection

| Mode | Planned interaction and evidence |
| --- | --- |
| CB | Region-specific channel list, scan/hold/resume, activity history, squelch and supported demodulation; no transmit control |
| AM/FM | Familiar frequency dial, seek, presets, supported station metadata and spectrum context; directory matches remain candidates until corroborated |
| Shortwave and other bands | Bounded band search, tuning bookmarks, time-of-day context and receiver limits; a quiet scan means no detected activity during observed intervals |
| Signal laboratory | Spectrum/waterfall, timing and packet inspection, Morse and supported decoders, replay and comparison; preserve unknown or failed interpretations |

All modes share service operations and CLI parity, keyboard navigation, plain text status and reduced-motion options. TUI styling can differ without changing permissions or starting work merely by opening a view. Seek/squelch thresholds are signal-detection controls with visible parameters, not proof of speech, a protocol or station identity. A model may propose an interpretation or bounded decoder experiment; service policy admits it.

Receiver location is optional and private by default. A nearby licensed transmitter and matching frequency suggest a candidate identity, not reception proof. Store the geographic basis and match evidence. Unusual signals can be bookmarked and reprocessed within retention limits; do not imply that arbitrary mathematics, encryption or unknown protocols can always be solved.

The initial hardware implementation exposes reception only, not a disabled transmit button that a theme or model can activate. Any later transmit proposal needs a separate capability and authorization design covering jurisdiction, band, device certification, power and emission limits. An acknowledgment or license checkbox is not a substitute for these constraints.
