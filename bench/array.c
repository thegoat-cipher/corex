/* C reference for bench/array.cx. */
#include <stdint.h>
#include <stdlib.h>

int main(void) {
    size_t n = 4000000;
    int64_t *p = malloc(n * sizeof(int64_t));
    for (size_t i = 0; i < n; i++) p[i] = (int64_t)i ^ 14979642;
    int64_t sum = 0;
    for (size_t i = 0; i < n; i++) sum += p[i];
    free(p);
    return (int)(sum % 256);
}
