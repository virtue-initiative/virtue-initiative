// See ../apple-cc. Only what `ring` and `libwebp` call.
#pragma once
#include <stddef.h>
void abort(void);
int abs(int x);
void *bsearch(const void *key, const void *base, size_t count, size_t size, int (*compare)(const void *, const void *));
void *calloc(size_t count, size_t size);
void free(void *ptr);
char *getenv(const char *name);
void *malloc(size_t size);
void qsort(void *base, size_t count, size_t size, int (*compare)(const void *, const void *));
