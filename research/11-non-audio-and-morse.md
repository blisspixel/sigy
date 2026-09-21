# Non-audio signals and Morse

Reviewed: 2026-09-20. Status: standards and upstream design research; no decoder or hardware tests.

## Evidence

Meshtastic's client interface and protobuf schemas represent structured messages and device events. A supported mesh source can enter Sigy at the packet/event layer without an audio stage. LoRa describes a radio modulation family; a usable packet protocol still needs its own adapter. [Client API](https://meshtastic.org/docs/development/device/client-api/), [protocol definitions](https://github.com/meshtastic/protobufs).

SigMF describes recorded sample data and metadata for signal interchange. It is a candidate for raw IQ preservation and replay, not a complete protocol decoder or Sigy job catalog. [SigMF](https://sigmf.org/).

GNU Radio distinguishes asynchronous messages from streams with sample-associated tags and provides conversion blocks. This supports evaluating separate event and sample paths instead of a single audio-only stream abstraction. It does not select GNU Radio as a runtime dependency. [Message passing](https://wiki.gnuradio.org/index.php/Message_Passing), [stream tags](https://wiki.gnuradio.org/index.php/Stream_Tags), [upstream PDU conversion declaration](https://github.com/gnuradio/gnuradio/blob/main/gr-pdu/grc/pdu_pdu_to_tagged_stream.block.yml). Direct wiki retrieval was unreliable; indexed official text and the upstream declaration were available.

rtl_433 is a concrete non-audio integration candidate: its receiver decodes device data and can emit structured events with timing and protocol metadata. Its upstream security guidance treats radio-derived output as untrusted. A Sigy adapter would validate fields and isolate the worker. Its documented hardware coverage does not prove compatibility with HackRF Pro. [Upstream project](https://github.com/merbanan/rtl_433).

ITU lists M.1677-1 as its in-force International Morse code recommendation. This is the baseline to pin for symbol and timing conformance before implementing a decoder. The recommendation page was verified; repeated PDF retrieval was unreliable during this research pass. [ITU M.1677-1](https://www.itu.int/rec/R-REC-M.1677-1-200910-I/).

Fldigi documents CW speed tracking, Farnsworth timing, and prosigns. SDRangel documents an audio route for Morse decoding and cautions that receive filtering affects the selected tone. These are useful behavior references, not evidence that either is the selected Sigy decoder. [Fldigi CW manual](https://www.w1hkj.org/FldigiHelp/cw_page.html), [SDRangel Morse audio workflow](https://github.com/f4exb/sdrangel/wiki/Decoding-Morse-code-from-audio).

## Proposed signal families

| Family | Stored observations | Examples of later capabilities |
| --- | --- | --- |
| Sample streams | Real/complex samples, format, rate, tuning, clock and gaps | Spectrum, channelization, demodulation, replay |
| Timed symbols | Keyed transitions, symbols, soft decisions, offsets | Morse timing inspection; modem decoding |
| Frames and packets | Original bytes/bits, framing, checksum/FEC result, protocol version | Mesh messages and protocol inspection |
| Structured events | Typed fields, units, source identity, decoder provenance | Telemetry plots, sensor thresholds, message search |
| Text | Original bytes/encoding, Unicode text, event linkage | Language identification and translation |
| Audio | Original media and derived PCM | Listening, speech, music |
| Measurements | Frequency/time bounds, value, units, calibration status | Spectral occupancy and signal-strength history |
| Opaque data | Original payload and known context | Retention and later reprocessing with an added decoder |

The design should permit adapters for other device transports and file formats. It must not promise arbitrary signals can be decoded. A decoder declares supported modulation/protocol variants, valid input ranges, and output types.

## Pipeline composition

One possible path is IQ -> channelization -> demodulation -> symbols -> framing/FEC -> protocol -> typed events. Other adapters begin at audio, packets, or text and omit unnecessary stages. Decryption belongs at the position specified by the actual protocol; it is not universally before or after framing.

Preserve clock mappings across resampling, channelization, and decoding. Original sample offsets, derived offsets, and device/receive times are distinct. Packet checksum success is not cryptographic authentication. Signal strength without calibration is not a precise physical measurement.

Raw-data retention, decoded-event retention, and analysis retention need independent policies. An IQ recording can be orders of magnitude larger than an audio recording. A lossy spectrum thumbnail cannot replace retained IQ when replay is promised.

Candidate decoder integrations include a supervised native worker, an existing application's supported headless interface, or an embedded library. Compare fault isolation, timing fidelity, resource bounds, packaging, licensing, and platform support. A C++ DSP dependency does not require selecting C++ for the application, and no Python product runtime is permitted.

## Morse scope and experience

Support received audio tones, later SDR-derived CW audio, and imported timed on/off observations. Keep audio conditioning, timing inference, symbol decoding, and text interpretation distinct.

Proposed controls include tone selection, filter bandwidth, automatic or fixed timing, speed bounds, and character-table/prosign profile. Preserve uncertain symbols and observed gaps. Adaptive timing must handle drift and hand-keyed variation without hiding low certainty.

The TUI can show an envelope or key-state timeline, mark and gap durations, dot/dash hypotheses, decoded characters, estimated speed, and the corresponding replay position. Stepping backward uses retained observations and decoder state. Manual corrections create a new revision.

Text-to-Morse encoding and local tone playback provide a useful practice mode. File-generated practice observations carry synthetic provenance and are excluded from collected-source findings by default. This feature does not imply enabling a transmitter.

Morse is an encoding. It may carry plain text, abbreviations, callsigns, or ciphertext. An alphabetic result is not necessarily a natural-language sentence. Historical and modern cryptographic operations are covered separately in [Cryptography](12-cryptography.md).

## Future evaluation

1. Verify international symbols, spacing, prosigns, empty input, and malformed sequences against the pinned standard.
2. Test machine-keyed and hand-keyed audio across speed, timing drift, Farnsworth spacing, amplitude changes, interference, frequency drift, clipping, and gaps.
3. Measure character/symbol error rates, missed and false detections, uncertainty usefulness, acquisition time, and compute cost.
4. Compare imported keyed events with audio decoding of equivalent fixtures. Keep independently annotated real recordings to avoid testing only against Sigy's own encoder.
5. Test bounded decoder queues and malformed packets while unrelated captures continue.
6. Replay the same retained IQ through different decoder versions and verify provenance and time alignment.
7. Test actual radios only when available, including tuning contention, sample drops, USB loss, and reconnects.

Morse and the workbench are confirmed product capabilities to plan. Their release placement is still open; they are not silently added as first-release blockers.
