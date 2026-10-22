/* C reference for bench/f64.cx. */
#include <stdint.h>
#include <stdlib.h>

int main(void) {
    size_t n = 2000000;
    double *a = malloc(n * sizeof(double));
    double *b = malloc(n * sizeof(double));
    for (size_t i = 0; i < n; i++) {
        a[i] = (double)i * 0.5;
        b[i] = (double)i * 0.25;
    }
    double sum = 0.0;
    for (size_t i = 0; i < n; i++) sum += a[i] * 1.5 + b[i];
    free(a);
    free(b);
    return (int)((int64_t)sum % 256);
}
