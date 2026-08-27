#!/usr/bin/env bash
set -euo pipefail

fixture_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/audio"
mkdir -p "$fixture_dir"

common=(
  -hide_banner
  -loglevel error
  -f lavfi
  -i anullsrc=r=44100:cl=stereo
  -t 0.25
  -map_metadata -1
  -fflags +bitexact
  -flags:a +bitexact
  -y
)

ffmpeg "${common[@]}" -c:a flac "$fixture_dir/seed.flac"
ffmpeg "${common[@]}" -c:a libmp3lame -b:a 96k "$fixture_dir/seed.mp3"
ffmpeg "${common[@]}" -c:a aac -b:a 96k "$fixture_dir/seed-aac.m4a"
ffmpeg "${common[@]}" -c:a alac "$fixture_dir/seed-alac.m4a"
ffmpeg "${common[@]}" -c:a libvorbis -q:a 2 "$fixture_dir/seed.ogg"
ffmpeg "${common[@]}" -c:a libopus -b:a 96k "$fixture_dir/seed.opus"

echo "Generated synthetic silent audio fixtures in $fixture_dir"
