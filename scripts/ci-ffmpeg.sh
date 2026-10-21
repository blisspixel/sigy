#!/bin/sh
# A pinned CI-only decoder profile. This does not install a product dependency.
# Curl's default retry does not cover a DNS failure, so each download is
# attempted three times. A failed attempt removes its partial file. The
# checksum and the signature still run once.
set -eu
fetch() {
  url=$1
  output=$2
  seconds=$3
  attempt=1
  while [ "$attempt" -le 3 ]; do
    if curl --proto '=https' --tlsv1.2 --fail --silent --show-error --location \
      --max-time "$seconds" "$url" -o "$output"; then
      return 0
    else
      status=$?
    fi
    rm -f "$output"
    if [ "$attempt" -eq 3 ]; then
      return "$status"
    fi
    sleep $((attempt * 5))
    attempt=$((attempt + 1))
  done
  return 1
}
task_root=$(mktemp -d "${RUNNER_TEMP:-/tmp}/sigy-ffmpeg.XXXXXX")
cd "$task_root"
fetch https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz source.tar.xz 120
printf '%s  %s\n' 8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e source.tar.xz | sha256sum --check
fetch https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz.asc source.asc 30
fetch https://ffmpeg.org/ffmpeg-devel.asc signing-key.asc 30
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
