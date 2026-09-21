# Hardware, LoRa, and SDR

Reviewed: 2026-09-20. Status: protocol and device-documentation research. No physical device tested.

## Meshtastic findings

Meshtastic exposes structured client traffic using protocol buffers over serial, TCP, and BLE. The transport protocol includes framing and configuration/state exchange. The application can integrate at that protocol boundary without selecting an implementation language based on one existing client SDK. [Client API](https://meshtastic.org/docs/development/device/client-api/).

Mesh payload visibility depends on channel configuration and the device's ability to decrypt it. Device clients can receive decrypted data from configured channels. Meshtastic traffic includes more than human-readable messages, and observing RF packets does not imply access to all message contents. [Encryption overview](https://meshtastic.org/docs/overview/encryption/).

## Proposed mesh integration

Start with a connected node and one well-tested transport. Serial is a candidate for the first hardware pilot, but no transport is selected until supported devices are identified. Add TCP and BLE according to measured reconnect behavior and platform support.

Preserve packet identity, sender identity as reported, receive time, channel context, payload type, raw payload reference, and available signal/route metadata. Device-reported identity and location are observations, not verified real-world identities.

Text translation bypasses speech recognition. Telemetry remains typed numeric/event data. Distinguish repeat reception from distinct messages; deduplication should preserve useful receive metadata. Device reboots and configuration changes create explicit events.

Generic LoRa support requires a named protocol and decoder. Supporting Meshtastic does not establish support for every LoRa application.

## HackRF Pro findings

The official HackRF Pro documentation lists a 100 kHz to 6 GHz operating range, half-duplex operation, and 8-bit I/Q at up to 20 million samples per second, with additional precision/rate modes. It describes compatibility goals with HackRF One software. These are device capabilities, not a Sigy compatibility result. [HackRF Pro](https://hackrf.readthedocs.io/en/latest/hackrf_pro.html).

The project documents platform-specific installation of host tools and libraries. Drivers, USB access, runtime libraries, and device firmware form part of the support matrix. [Software installation](https://hackrf.readthedocs.io/en/latest/installing_hackrf_software.html).

SigMF describes sample datasets and accompanying metadata including sample representation, sample rate, capture frequency/time, and annotations. It is a strong candidate for interoperable IQ recordings and replay fixtures. [SigMF specification](https://sigmf.org/).

## Proposed SDR integration

Build a receive-only pilot: enumerate a device, reserve it, configure a known receive profile, acquire bounded samples, inspect spectrum, demodulate a supported broadcast, and record metadata. Preserve tuning, gain, sample rate/format, hardware identity, driver/runtime versions, and dropped-sample events.

One acquisition has a tuning configuration. Independent monitors cannot silently retune the same device. Multiple channels may share a sampled band only when the capture backend and channelizer support that topology. The scheduler reports conflicts before collection begins.

Keep spectrum display decimation out of the acquisition path. A slow terminal must not force sample loss. IQ retention is separately budgeted; at 20 million samples/s with one byte each for I and Q, raw data is about 144 GB/hour before overhead.

An SDR audio demodulator becomes an audio producer for the existing archive and processing pipeline. It also retains the RF provenance needed to explain how that audio was obtained.

## Adapter boundary alternatives

| Boundary | Advantage | Main cost |
| --- | --- | --- |
| Host tool/worker process | Isolation and simpler driver failure containment | Process supervision, sample transport, extra copies |
| Direct native library | Precise control and potentially lower-copy sample transfer | Unsafe/native boundary, ABI and ownership review |
| General device abstraction | Potential reuse across radio families | Lowest-common capability limits and additional dependency behavior |

The hardware pilot should measure these tradeoffs on the actual target device rather than assume a general abstraction exposes every advanced mode.

## Hardware acceptance plan

Test discovery, permission errors, one-device ownership, unplug/replug, device reset, sample discontinuity, queue overload, conflicting monitor requests, and cancellation. Replay known IQ files before physical testing, then compare physical observations with a reference tool.

Record tested combinations of device revision, firmware, host OS/architecture, driver, and backend. Unsupported combinations remain experimental.

## Near future

Plan additional radio families and protocols through capability contracts and replay fixtures. Transmission, firmware flashing, direction finding, distributed receivers, and device-specific advanced modes require separate scoped designs.
