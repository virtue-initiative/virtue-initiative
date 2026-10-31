// See ../apple-cc. `libwebp` only logs with these. FILE is opaque, and __stderrp is libSystem's name
// for stderr.
#pragma once
#include <stddef.h>
typedef struct __sFILE FILE;
extern FILE *__stderrp;
#define stderr __stderrp
int fclose(FILE *stream);
FILE *fopen(const char *path, const char *mode);
int fprintf(FILE *stream, const char *format, ...);
int printf(const char *format, ...);
int snprintf(char *str, size_t size, const char *format, ...);
