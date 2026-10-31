// See ../apple-cc.
#pragma once
#ifdef NDEBUG
#define assert(condition) ((void)0)
#else
#define assert(condition) ((condition) ? (void)0 : __builtin_trap())
#endif
