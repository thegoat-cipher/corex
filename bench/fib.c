/* C reference for bench/fib.cx. */

#include <stdint.h>

static uint32_t fib(uint32_t n) {
    if (n < 2) return n;
    return fib(n - 1) + fib(n - 2);
}

int main(void) {
    return (int)(fib(35) % 256);
}
