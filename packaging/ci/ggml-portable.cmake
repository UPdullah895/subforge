# Toolchain file handed to whisper.cpp's CMake build through `CMAKE_TOOLCHAIN_FILE`
# (whisper-rs-sys forwards every CMAKE_* environment variable as a CMake define).
#
# Why this exists: with GGML_NATIVE=ON (the default) ggml compiles with `-march=native`,
# so the binary inherits the CPU of whatever machine built it. The release runners are
# AVX-512 capable, which turned v0.3.0 into an AVX-512-only build: on any CPU without
# AVX-512 (Ryzen 5000 and older, Intel 12th-14th gen, …) loading a whisper model died with
# SIGILL inside ggml_cpu. Nothing caught it because CI also runs on AVX-512 runners.
#
# Instead of native, ask for one portable baseline: x86-64 with AVX2/FMA/F16C (Haswell
# 2013+, Excavator 2015+) and nothing newer. AVX-512 stays off, so the same binary runs on
# every machine the app claims to support.
set(GGML_NATIVE OFF CACHE BOOL "ggml: enable -march=native flag (disabled for portable release builds)" FORCE)
set(GGML_AVX512 OFF CACHE BOOL "ggml: AVX-512 (needs Skylake-X / Zen 4+)" FORCE)
set(GGML_AVX512_VBMI OFF CACHE BOOL "" FORCE)
set(GGML_AVX512_VNNI OFF CACHE BOOL "" FORCE)
set(GGML_AVX512_BF16 OFF CACHE BOOL "" FORCE)
set(GGML_AVX512_FP16 OFF CACHE BOOL "" FORCE)
set(GGML_AVX2 ON CACHE BOOL "ggml: AVX2 (Haswell 2013+)" FORCE)
set(GGML_FMA ON CACHE BOOL "" FORCE)
set(GGML_F16C ON CACHE BOOL "" FORCE)
