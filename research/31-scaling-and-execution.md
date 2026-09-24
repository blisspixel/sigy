# Scaling and execution research

Reviewed: 2026-09-24. Sources were read on that date; crate versions come from the crates.io API. This record supports the [scaling architecture](../docs/design/scaling-architecture.md). Figures marked secondary need a primary check before they back a price snapshot.

## Job queues

| Option | Semantics | Rust client | Assessment |
| --- | --- | --- | --- |
| SQLite (current) | One writer, exact generations, durable read leases | rusqlite (vendored) | Keep as the single-machine default |
| Postgres `FOR UPDATE SKIP LOCKED` | At-least-once with a lease column; the [PostgreSQL 18 `SELECT` documentation](https://www.postgresql.org/docs/current/sql-select.html) describes it for queue-like tables and warns of an inconsistent view | tokio-postgres 0.7.18, sqlx 0.9.0 | Recommended server-grade queue: claim, fence, publish and settle in one transaction |
| pgmq, graphile_worker, apalis-postgres | Library-owned schemas | pgmq 0.33.7; graphile_worker 0.13.5; apalis-postgres 1.0.0-rc.9 | Not adopted; the schema must hold Sigy's generations |
| NATS JetStream | At-least-once, bounded deduplication window ([NATS](https://docs.nats.io/learn/jetstream/delivery-and-acknowledgment)) | async-nats 0.50.0 | Notifications only, never the job authority |
| Redis Streams | At-least-once with idle reclaim ([XAUTOCLAIM](https://redis.io/docs/latest/commands/xautoclaim/)) | redis 1.7.0 | Not adopted |
| Kafka share groups | Queue semantics on a log ([Confluent](https://www.confluent.io/blog/kafka-queue-semantics-share-consumer-ga/), secondary) | rdkafka 0.39.0 (native library) | Too heavy |
| RabbitMQ quorum queues | At-least-once, delivery limits, ack timeout ([RabbitMQ](https://www.rabbitmq.com/docs/consumers)) | lapin 4.12.0 | Not adopted |
| Temporal, Restate, Inngest | Durable execution with their own replay models | temporalio-sdk 1.0.0 (public preview per its changelog); restate-sdk 0.12.1 (server under BSL 1.1); inngest 0.1.1 (stale) | Not adopted: a second runtime and job model |

Generations act as fencing tokens, following [Kleppmann, "How to do distributed locking" (2016)](https://martin.kleppmann.com/2016/02/08/how-to-do-distributed-locking.html). Lease deadlines use the database clock. Paid attempts are never redelivered.

## Execution isolation

- **Rootless Podman** gives `--network none`, a read-only root, dropped capabilities, seccomp and resource limits; resource flags require cgroup v2 when rootless ([podman-run](https://docs.podman.io/en/latest/markdown/podman-run.1.html)). NVIDIA GPUs attach through CDI ([NVIDIA](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/1.16.0/cdi-support.html)); AMD through `/dev/kfd` and `/dev/dri`.
- **gVisor** supports NVIDIA only and says it is less effective against driver vulnerabilities ([gVisor GPU](https://gvisor.dev/docs/user_guide/gpu/)). **Kata** gives a VM boundary with whole-GPU passthrough ([NVIDIA GPU Operator](https://docs.nvidia.com/datacenter/cloud-native/gpu-operator/latest/deploy-kata-containers.html)).
- **Windows containers** accelerate DirectX only in process isolation ([Microsoft Learn](https://learn.microsoft.com/en-us/virtualization/windowscontainers/deploy-containers/gpu-acceleration)), so the Job Object stays the Windows baseline; Linux containers in WSL2 support NVIDIA CUDA ([CUDA on WSL](https://docs.nvidia.com/cuda/wsl-user-guide/index.html)).
- **macOS**: Apple `container` runs one VM per container without GPU access ([apple/container](https://github.com/apple/container)); Podman with krunkit exposes Vulkan compute through Venus ([Red Hat, 2025-06-05](https://developers.redhat.com/articles/2025/06/05/how-we-improved-ai-inference-macos-podman-containers)).
- **WASM** (wasmtime) gives strong CPU-only bounds ([ResourceLimiter](https://docs.wasmtime.dev/api/wasmtime/trait.ResourceLimiter.html)) but no practical GPU path.
- **Kubernetes** 1.36 made OCI volume sources generally available ([KEP-4639](https://github.com/kubernetes/enhancements/issues/4639); [bex.co, 2026-07-11](https://bex.co/blog/2026/07/11/kubernetes-136-oci-volumesource-build-artifacts), secondary), so models can be mounted as artifacts whose digest equals the model hash.

## Serverless GPU and hosted providers

| Platform | Billing | Maximum duration | Notes |
| --- | --- | --- | --- |
| RunPod Serverless | Per second, including start and idle ([pricing](https://docs.runpod.io/serverless/pricing)) | Configurable execution timeout ([requests](https://docs.runpod.io/serverless/endpoints/send-requests)) | Hourly account cap |
| Modal | Per second ([pricing](https://modal.com/pricing)) | 1 s to 24 h, may overrun by seconds ([timeouts](https://modal.com/docs/guide/timeouts)) | Monthly workspace budget ([budgets](https://modal.com/docs/guide/budgets)) |
| Google Cloud Run jobs | Per second of instance lifetime | 1 h for GPU tasks ([GPU jobs](https://docs.cloud.google.com/run/docs/configuring/jobs/gpu)) | |
| Azure Container Apps | Per GPU-second ([Learn](https://learn.microsoft.com/en-us/azure/container-apps/gpu-serverless-overview)) | Not documented | |
| AWS SageMaker async | Instance time | Up to 3,600 s ([API](https://docs.aws.amazon.com/sagemaker/latest/APIReference/API_runtime_InvokeEndpointAsync.html)) | |
| Fly GPUs | Retired 2026-08-01 ([Fly community](https://community.fly.io/t/gpu-migration-fly-io-gpus-will-be-deprecated-as-of-july-31-2026/27110)) | | Excluded |

None exposes an authoritative per-request charge, so a remote task stays an uncertain liability until usage exports reconcile it. Hosted speech services billed by audio duration give an exact prior bound: Groq `whisper-large-v3-turbo` at USD 0.04 per hour with a 10 s minimum ([Groq](https://console.groq.com/docs/speech-to-text)); Deepgram and ElevenLabs rates are secondary. OpenAI transcription models are token-billed ([OpenAI pricing](https://developers.openai.com/api/docs/pricing)) and need the provider token-bound rules. OpenRouter key limits with `limit_reset: null` give a non-refilling backstop ([limits](https://openrouter.ai/docs/api/reference/limits)).

## Codebase findings

- Admission and dispatch are one step today, so a busy worker slot refuses work instead of queueing it.
- One active job per kind is enforced by SQL unique indexes.
- Job history is capped at 256 rows per table. With 60-second recognition jobs, one continuous station would reach that cap in about four hours, so the cap must become a bound on active and queued work.
- Workers receive host paths; content-addressed staging removes that.
- The drain proof is local; remote executors need fenced containment.

## Evidence still needed

Rootless Podman runs of whisper.cpp and llama.cpp with `--network none` and GPU devices; the WSL2 path for AMD integrated graphics; the Vulkan-in-VM path on macOS; invoice reconciliation for each remote platform before any price snapshot is trusted.
