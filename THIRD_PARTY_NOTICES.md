# Third-party notices

SubForge is licensed under GPL-3.0-only (see `LICENSE`). The Windows installer and the AppImage
also contain the programs and libraries below, unmodified, as separate files. Their licence
texts are installed in the `licenses/` folder next to them (one sub-folder per tool, with a
`SOURCE.txt` naming the exact download). Versions, download URLs and checksums are pinned in
`packaging/bundled-tools.json`.

The `.deb`, `.rpm` and AUR packages bundle none of these; they depend on the distribution's
`ffmpeg`, `mkvtoolnix` and Vulkan loader packages instead.

## Source code (GPL/LGPL)

Every release on https://github.com/UPdullah895/subforge/releases has the corresponding source
archives of the GPL/LGPL components below attached (FFmpeg, MKVToolNix, Qt, GLib, GnuTLS,
Nettle, libidn2, libunistring, libtasn1, libdvdread), the exact versions listed in the
`sources` section of `packaging/bundled-tools.json`. Anyone can fetch and verify the same files
with `node scripts/fetch-bundled-tools.mjs --sources <folder>`.

**Written offer.** Some components are built into the bundled binaries by their upstream
builders and are not attached individually: the static libraries inside the FFmpeg builds
(for example x264, x265, libvpx, dav1d and the others listed by `ffmpeg -version`) and the
libraries statically linked into the Windows MKVToolNix programs. For at least three years
after we last distribute a given SubForge release, we will give anyone who asks a copy of the
complete corresponding source code of every GPL/LGPL component in that release, on a medium
customarily used for software interchange, for no more than our cost of providing it. Ask by
opening an issue at https://github.com/UPdullah895/subforge/issues and name the release.

## FFmpeg (GPL build)

- Windows: gyan.dev "essentials" build 9.0.2, GPL-3.0-or-later. Builds:
  https://www.gyan.dev/ffmpeg/builds/ (release mirror: https://github.com/GyanD/codexffmpeg).
  Source: `ffmpeg-9.0.2.tar.xz`; the build configuration is in `licenses/ffmpeg/README.txt`.
- Linux (AppImage): BtbN static "linux64-gpl" build n9.0.2-17-g2a571b6068, GPL-3.0-or-later.
  Builds and build scripts: https://github.com/BtbN/FFmpeg-Builds. Source: FFmpeg commit
  2a571b606854520cf89804d8030c8b328e621689.
- Project: https://ffmpeg.org/ (licence: https://ffmpeg.org/legal.html)
- Licence files: `licenses/ffmpeg/`

## MKVToolNix 103.0 (mkvmerge, mkvextract, mkvpropedit)

- GPL-2.0-only. Copyright Moritz Bunkus and contributors. https://mkvtoolnix.download/
  Source: `mkvtoolnix-103.0.tar.xz` (https://codeberg.org/mbunkus/mkvtoolnix).
- Windows: the official portable 64-bit build; its licence texts (GPL-2.0, LGPL-2.1, LGPL-3.0,
  Boost, CC-BY-3.0, and the MIT texts of nlohmann-json, pugixml and QtWaitingSpinner) are
  installed in `licenses/mkvtoolnix/`.
- Linux (AppImage): the command-line tools and the shared libraries they load, taken unmodified
  from the official AppImage (built on an Enterprise Linux 8 base). Each library's licence text
  is in `licenses/mkvtoolnix/<library>-<version>/`:

  | Library (file) | Version | Licence | Project |
  |---|---|---|---|
  | Qt Core (`libQt6Core.so.6`) | 6.6.1 | LGPL-3.0-only | https://www.qt.io/ |
  | Boost (`libboost_atomic`, `libboost_filesystem`) | 1.85.0 | BSL-1.0 | https://www.boost.org/ |
  | ICU (`libicudata`, `libicui18n`, `libicuuc` .so.56) | 56.1 | ICU licence | https://icu.unicode.org/ |
  | GLib (`libglib-2.0`, `libgthread-2.0`) | 2.56.4 | LGPL-2.1-or-later | https://gitlab.gnome.org/GNOME/glib |
  | GnuTLS (`libgnutls.so.30`) | 3.6.16 | LGPL-2.1-or-later | https://gnutls.org/ |
  | Nettle (`libnettle.so.6`, `libhogweed.so.4`) | 3.4.1 | LGPL-3.0-or-later or GPL-2.0-or-later | https://www.lysator.liu.se/~nisse/nettle/ |
  | libidn2 (`libidn2.so.0`) | 2.2.0 | LGPL-3.0-or-later or GPL-2.0-or-later | https://gitlab.com/libidn/libidn2 |
  | libunistring (`libunistring.so.2`) | 0.9.9 | LGPL-3.0-or-later or GPL-2.0-or-later | https://www.gnu.org/software/libunistring/ |
  | libtasn1 (`libtasn1.so.6`) | 4.13 | LGPL-2.1-or-later | https://www.gnu.org/software/libtasn1/ |
  | libdvdread (`libdvdread.so.4`) | 5.0.3 | GPL-2.0-or-later | https://code.videolan.org/videolan/libdvdread |
  | p11-kit (`libp11-kit.so.0`) | 0.23.22 | BSD-3-Clause | https://github.com/p11-glue/p11-kit |
  | PCRE (`libpcre.so.1`) | 8.42 | BSD-3-Clause | https://www.pcre.org/ |
  | libffi (`libffi.so.6`) | 3.1 | MIT | https://sourceware.org/libffi/ |
  | FLAC (`libFLAC.so.8`) | 1.3.2 | BSD-3-Clause (Xiph) | https://xiph.org/flac/ |
  | libogg (`libogg.so.0`) | 1.3.2 | BSD-3-Clause | https://xiph.org/ogg/ |
  | libvorbis (`libvorbis.so.0`) | 1.3.6 | BSD-3-Clause | https://xiph.org/vorbis/ |
  | cmark (`libcmark.so.0.28.3`) | 0.28.3 | BSD-2-Clause (and others, see COPYING) | https://github.com/commonmark/cmark |

  The libraries are Enterprise Linux 8 builds of these upstream versions (with that
  distribution's patches); the attached sources are the upstream releases.
- **Host requirements.** Like the official AppImage, these binaries also use libraries from the
  host system that are not bundled: glibc 2.28 or newer, libstdc++, libgcc_s, libz and libgmp.

## Vulkan Loader

- Apache-2.0. Copyright The Khronos Group Inc., Valve Corporation, LunarG, Inc.
  https://github.com/KhronosGroup/Vulkan-Loader
- Windows: `vulkan-1.dll` (x64) 1.4.304 from the LunarG Vulkan Runtime components
  (https://vulkan.lunarg.com/sdk/home), installed next to `subforge.exe`.
- Linux (AppImage): `libvulkan.so.1` 1.3.204.1 from Ubuntu 22.04's `libvulkan1` package
  (https://packages.ubuntu.com/jammy/libvulkan1). It finds the system's GPU drivers as usual.
- Licence files: `licenses/vulkan-loader/`

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
