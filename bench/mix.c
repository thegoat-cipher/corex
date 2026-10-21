/* C reference for bench/mix.cx. Same parsing, same recurrence. */

#include <stdint.h>
#include <unistd.h>

static uint64_t read_n(void) {
    unsigned char text[12] = {0};
    if (read(0, text, sizeof text) < 0) return 0;
    uint64_t n = 0;
    for (int k = 0; k < 12; k++) {
        unsigned char digit = text[k];
        if (digit < 48 || digit > 57) break;
        n = n * 10 + (uint64_t)(digit - 48);
    }
    return n;
}

int main(void) {
    uint64_t n = read_n();
    uint64_t state = 0x2545F4914F6CDD1DULL;
    for (uint64_t i = 0; i < n; i++) {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        state = state ^ (state >> 17);
        state = state + i;
    }
    return (int)(state % 256);
}
