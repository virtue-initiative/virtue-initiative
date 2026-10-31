// See ../apple-cc. `ring` includes this only to tell macOS from iOS.
#pragma once
#if defined(__ENVIRONMENT_IPHONE_OS_VERSION_MIN_REQUIRED__)
#define TARGET_OS_IPHONE 1
#define TARGET_OS_OSX 0
#else
#define TARGET_OS_IPHONE 0
#define TARGET_OS_OSX 1
#endif
#define TARGET_OS_MAC 1
