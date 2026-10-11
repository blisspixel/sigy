#!/bin/sh
# A pinned CI-only decoder profile. This does not install a product dependency.
set -eu
task_root=$(mktemp -d "${RUNNER_TEMP:-/tmp}/sigy-ffmpeg.XXXXXX")
cd "$task_root"
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
  --max-time 120 https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz -o source.tar.xz
printf '%s  %s\n' 8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e source.tar.xz | sha256sum --check
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --max-time 30 \
  https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz.asc -o source.asc
curl --proto '=https' --tlsv1.2 --fail --silent --show-error --max-time 30 \
  https://ffmpeg.org/ffmpeg-devel.asc -o signing-key.asc
mkdir -m 700 keyring
gpg --batch --homedir "$task_root/keyring" --import signing-key.asc
gpg --batch --homedir "$task_root/keyring" --status-fd 1 --verify source.asc source.tar.xz > signature.status
awk '$1 == "[GNUPG:]" && $2 == "VALIDSIG" && $3 == "FCF986EA15E6E293A5644F10B4322F04D67658D8" { valid=1 } END { exit !valid }' signature.status
tar -xJf source.tar.xz
cd ffmpeg-9.0.2
./configure --disable-everything --disable-autodetect --disable-doc --disable-debug \
  --disable-x86asm --disable-network --disable-ffplay --disable-ffprobe \
  --enable-ffmpeg --enable-protocol=file,pipe --enable-demuxer=wav \
  --enable-decoder=pcm_u8,pcm_s16le --enable-encoder=pcm_s16le \
  --enable-muxer=null --enable-filter=anull,aresample,atrim,asetpts
make -j2
printf 'SIGY_TEST_FFMPEG=%s/ffmpeg\n' "$PWD" >> "$GITHUB_ENV"
