#!/usr/bin/env bash
# MSYS2 UCRT64 half of build-uxplay.ps1. Expects UXPLAY_REF, UXPLAY_WORK, UXPLAY_OUT.
set -euo pipefail

: "${UXPLAY_REF:?}" "${UXPLAY_WORK:?}" "${UXPLAY_OUT:?}"
P=mingw-w64-ucrt-x86_64

pacman -S --needed --noconfirm git binutils \
  $P-gcc $P-cmake $P-ninja $P-pkgconf $P-libplist $P-openssl \
  $P-gstreamer $P-gst-plugins-base $P-gst-plugins-good \
  $P-gst-plugins-bad $P-gst-libav

SRC="$UXPLAY_WORK/src"
if [ ! -d "$SRC/.git" ]; then
  git clone https://github.com/FDH2/UxPlay.git "$SRC"
fi
git -C "$SRC" fetch --force origin
git -C "$SRC" checkout --force "$UXPLAY_REF"

# No -DUSE_DNS_SD=1: pre-1.74 master uses the built-in lib/mdnsd (no Bonjour).
rm -rf "$SRC/build"
mkdir "$SRC/build"
cd "$SRC/build"
cmake -G Ninja -DCMAKE_BUILD_TYPE=Release -DNO_MARCH_NATIVE=ON ..
ninja

PREFIX="$(cygpath -u "$MSYSTEM_PREFIX")"   # /ucrt64
rm -rf "$UXPLAY_OUT"/*.exe "$UXPLAY_OUT"/*.dll "$UXPLAY_OUT"/gst-plugins "$UXPLAY_OUT"/LICENSE*
mkdir -p "$UXPLAY_OUT/gst-plugins"
cp uxplay.exe "$UXPLAY_OUT/"
cp "$SRC/LICENSE" "$UXPLAY_OUT/LICENSE-UxPlay.txt"

# GStreamer plugins UxPlay's pipelines need (video: h264parse+avdec_h264+d3d11/gl sinks;
# audio: avdec_aac/alac + directsound/wasapi sinks; plus glue elements).
PLUGINS="coreelements app playback videoconvertscale videoparsersbad \
 audioconvert audioresample audioparsers volume videorate autodetect libav \
 d3d11 d3d12 directsound wasapi wasapi2 opengl debugutilsbad \
 videofilter isomp4 gio level pango typefindfunctions jpeg audiofx rawparse subparse"
for p in $PLUGINS; do
  f="$PREFIX/lib/gstreamer-1.0/libgst$p.dll"
  if [ -f "$f" ]; then cp "$f" "$UXPLAY_OUT/gst-plugins/"; else echo "note: plugin $p not present, skipped"; fi
done

# Copy every non-system DLL the exe and the plugins depend on (transitive).
declare -A seen
queue=("$UXPLAY_OUT/uxplay.exe" "$UXPLAY_OUT"/gst-plugins/*.dll)
while [ ${#queue[@]} -gt 0 ]; do
  cur="${queue[0]}"; queue=("${queue[@]:1}")
  while read -r dll; do
    [ -z "$dll" ] && continue
    [ -n "${seen[$dll]:-}" ] && continue
    seen[$dll]=1
    src="$PREFIX/bin/$dll"
    if [ -f "$src" ]; then
      cp -n "$src" "$UXPLAY_OUT/"
      queue+=("$UXPLAY_OUT/$dll")
    fi  # else: a Windows system DLL (kernel32, d3d11, ...), not bundled
  done < <(objdump -p "$cur" | sed -n 's/.*DLL Name: //p')
done

# GStreamer's plugin-scanner helper (optional; the app sets GST_REGISTRY_FORK=no).
echo "Vendored $(ls "$UXPLAY_OUT"/*.dll | wc -l) DLLs and $(ls "$UXPLAY_OUT"/gst-plugins | wc -l) plugins."
