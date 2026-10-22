/* C reference for bench/struct.cx. */
#include <stdint.h>
#include <stdlib.h>

typedef struct {
    int64_t key;
    int64_t value;
    uint32_t tag;
    uint32_t pad;
} Item;

int main(void) {
    size_t n = 2000000;
    Item *items = malloc(n * sizeof(Item));
    for (size_t i = 0; i < n; i++) {
        items[i].key = (int64_t)i;
        items[i].value = (int64_t)i * 3;
        items[i].tag = 1;
        items[i].pad = 0;
    }
    int64_t sum = 0;
    for (size_t i = 0; i < n; i++) {
        items[i].value += items[i].key;
        sum += items[i].value;
    }
    free(items);
    return (int)(sum % 256);
}
