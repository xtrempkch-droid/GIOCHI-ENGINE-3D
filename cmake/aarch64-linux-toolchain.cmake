# CMake toolchain file used when cross-compiling the bundled SDL2 for
# aarch64-unknown-linux-gnu.
#
# Without it, SDL2's CMake project configures as a *native* build and probes the
# host for optional libraries; the resulting `-lsamplerate`-style flags point at
# x86_64 sonames that the aarch64 linker rejects.
#
# The aarch64 sysroot is taken from CROSS_SYSROOT when set (see
# docs/CROSS_COMPILE.md); otherwise the compiler's default sysroot is used.
set(CMAKE_SYSTEM_NAME Linux)
set(CMAKE_SYSTEM_PROCESSOR aarch64)

set(CMAKE_C_COMPILER aarch64-linux-gnu-gcc)
set(CMAKE_CXX_COMPILER aarch64-linux-gnu-g++)

if(DEFINED ENV{CROSS_SYSROOT})
  set(CMAKE_SYSROOT "$ENV{CROSS_SYSROOT}")
  # CMAKE_SYSROOT alone only changes the compiler's default search paths; it does
  # not restrict `find_library`/`find_package`. Without this line CMake still walks
  # the host's /usr/lib/x86_64-linux-gnu, finds libEGL/libpulse/libwayland there,
  # and hands the aarch64 linker flags that resolve to x86_64 sonames.
  set(CMAKE_FIND_ROOT_PATH "$ENV{CROSS_SYSROOT}")
endif()

# Optional SDL2 dependencies must not be resolved from the host system when the
# cross sysroot does not provide them; SDL2 degrades gracefully without them.
set(SDL_LIBSAMPLERATE OFF CACHE BOOL "" FORCE)
set(SDL_LIBUDEV OFF CACHE BOOL "" FORCE)

set(CMAKE_FIND_ROOT_PATH_MODE_PROGRAM NEVER)
set(CMAKE_FIND_ROOT_PATH_MODE_LIBRARY ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_INCLUDE ONLY)
set(CMAKE_FIND_ROOT_PATH_MODE_PACKAGE ONLY)
