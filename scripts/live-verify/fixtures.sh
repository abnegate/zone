#!/bin/sh
# Media the live suite drives the console with, built from ffmpeg rather than
# vendored: every lane is asserted byte-exact against the file the ComfyUI
# stand-in served, so what matters is that both sides read the same bytes.
#
#   ./scripts/live-verify/fixtures.sh <directory>
set -eu

out=${1:?usage: fixtures.sh <directory>}
media="$out/media"
training="$out/training"
mkdir -p "$media" "$training"

command -v ffmpeg >/dev/null || {
  echo "ffmpeg is required" >&2
  exit 1
}

# One file per lane, each visibly different from the others so a lane that
# collects the wrong one fails rather than passing on a coincidence.
ffmpeg -y -loglevel error -f lavfi -i "color=c=0x3366cc:s=512x512:d=1" \
  -frames:v 1 "$media/image.png"
ffmpeg -y -loglevel error -f lavfi -i "color=c=0xcc3366:s=1024x1024:d=1" \
  -frames:v 1 "$media/upscaled.png"
ffmpeg -y -loglevel error -f lavfi -i "testsrc=s=320x240:d=2:r=12" \
  -c:v libvpx-vp9 -b:v 200k -pix_fmt yuv420p "$media/video.webm"
ffmpeg -y -loglevel error -f lavfi -i "testsrc=s=640x480:d=2:r=12" \
  -c:v libvpx-vp9 -b:v 300k -pix_fmt yuv420p "$media/upscaled.webm"
ffmpeg -y -loglevel error -f lavfi -i "sine=frequency=440:duration=3" \
  -c:a flac "$media/audio.flac"

# The clip the Train tab samples. Long enough that selection has something to
# choose between, and moving, so mirrored frames are distinguishable.
ffmpeg -y -loglevel error -f lavfi -i "testsrc=s=640x360:d=6:r=8" \
  -c:v libx264 -pix_fmt yuv420p -movflags +faststart "$media/training.mp4"

# A training set screening has to have opinions about: six distinct frames, an
# exact repeat of one, a thumbnail, and a smeared frame.
rm -f "$training"/*.png
ffmpeg -y -loglevel error -f lavfi -i "testsrc2=s=1024x1024:d=8:r=1" \
  -fps_mode passthrough "$training/frame-%02d.png"
i=1
while [ "$i" -le 6 ]; do
  mv "$training/frame-0$i.png" "$training/subject-$i.png"
  i=$((i + 1))
done
rm -f "$training"/frame-*.png
cp "$training/subject-2.png" "$training/duplicate-of-2.png"
ffmpeg -y -loglevel error -i "$training/subject-3.png" -vf scale=128:128 "$training/tiny.png"
ffmpeg -y -loglevel error -i "$training/subject-4.png" -vf "boxblur=24:2" "$training/smeared.png"

echo "fixtures written to $out"
