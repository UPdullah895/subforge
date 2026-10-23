# Third-party notices

SubForge is licensed under GPL-3.0-only (see `LICENSE`). The Windows installer and the AppImage
also contain the programs below, unmodified, as separate executables. Their licence texts are
installed in the `licenses/` folder next to them (one sub-folder per tool, with a `SOURCE.txt`
naming the exact download). Versions and checksums are pinned in `packaging/bundled-tools.json`.

The `.deb`, `.rpm` and AUR packages bundle none of these; they depend on the distribution's
`ffmpeg`, `mkvtoolnix` and Vulkan loader packages instead.

## FFmpeg (GPL build)

- Windows: gyan.dev "essentials" build, GPL-3.0-or-later.
  Builds: https://www.gyan.dev/ffmpeg/builds/ (release mirror: https://github.com/GyanD/codexffmpeg)
- Linux (AppImage): BtbN static "linux64-gpl" build, GPL-3.0-or-later.
  Builds: https://github.com/BtbN/FFmpeg-Builds
- Source code: https://ffmpeg.org/download.html and https://git.ffmpeg.org/ffmpeg.git; the build
  configuration of each binary is printed by `ffmpeg -version`, and the build scripts are in the
  repositories above. Licence: https://ffmpeg.org/legal.html
- Installed licence files: `licenses/ffmpeg/`

## MKVToolNix (mkvmerge, mkvextract, mkvpropedit)

- GPL-2.0-only. Copyright Moritz Bunkus and contributors. https://mkvtoolnix.download/
- Source code: https://mkvtoolnix.download/source.html and https://codeberg.org/mbunkus/mkvtoolnix
- Windows: the official portable 64-bit build. Linux: the command-line tools and the shared
  libraries they load, taken unmodified from the official AppImage: Qt 6 Core (LGPL-3.0),
  Boost (BSL-1.0), ICU (Unicode licence), GLib (LGPL-2.1+), GnuTLS (LGPL-2.1+), Nettle/Hogweed
  (LGPL-3.0+ or GPL-2.0+), libidn2 and libunistring (LGPL-3.0+ or GPL-2.0+), libtasn1
  (LGPL-2.1+), p11-kit (BSD-3-Clause), PCRE (BSD-3-Clause), libffi (MIT), FLAC, libogg and
  libvorbis (BSD-3-Clause), libdvdread (GPL-2.0+) and cmark (BSD-2-Clause). Their sources are
  linked from the MKVToolNix source page above.
- Installed licence files: `licenses/mkvtoolnix/`

## Vulkan Loader

- Apache-2.0. Copyright The Khronos Group Inc., Valve Corporation, LunarG, Inc.
  https://github.com/KhronosGroup/Vulkan-Loader
- Windows: `vulkan-1.dll` (x64) from the LunarG Vulkan Runtime components
  (https://vulkan.lunarg.com/sdk/home), installed next to `subforge.exe`.
- Linux (AppImage): `libvulkan.so.1` from Ubuntu 22.04's `libvulkan1` package
  (https://packages.ubuntu.com/jammy/libvulkan1). It finds the system's GPU drivers as usual.
- Installed licence files: `licenses/vulkan-loader/`

## whisper.cpp / ggml

- MIT. Copyright The ggml authors. https://github.com/ggml-org/whisper.cpp
- Compiled into SubForge through the `whisper-rs` crate (Unlicense)
  (https://codeberg.org/tazz4843/whisper-rs). Speech models are downloaded on demand from
  https://huggingface.co/ggerganov/whisper.cpp (MIT).

## Noto Naskh Arabic

- SIL Open Font License 1.1. Copyright 2022 The Noto Project Authors.
  https://github.com/notofonts/arabic
- Named by the synthetic Arabic test subtitles; the licence text is in
  `crates/subforge-core/tests/fixtures/subtitles/fonts/NotoNaskhArabic-OFL.txt`.
