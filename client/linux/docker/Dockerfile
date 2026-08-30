# Build environment for the Linux .deb, pinned to Debian oldstable
# (bookworm). Building here rather than on a newer distro means the
# resulting binary links against older glibc/libstdc++/system library
# versions, which are forward-compatible: a binary built against an old
# glibc runs fine on newer Debian/Ubuntu releases, but not the reverse. This
# gives the packaged .deb the widest install compatibility.
#
# Bump the base image tag when Debian's oldstable moves to a new release;
# pinned explicitly (rather than following the floating `oldstable` tag) so
# builds stay reproducible until that's a deliberate decision.
FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    libleptonica-dev \
    libtesseract-dev \
    tesseract-ocr-eng \
    libclang-dev \
    clang \
    patchelf \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace
