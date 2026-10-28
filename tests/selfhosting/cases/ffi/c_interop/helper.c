typedef struct { long a; long b; } Pair;

long c_add(long a, long b) { return a + b; }

long c_sum(int count, ...) {
    __builtin_va_list ap;
    __builtin_va_start(ap, count);
    long total = 0;
    for (int i = 0; i < count; i++) total += __builtin_va_arg(ap, long);
    __builtin_va_end(ap);
    return total;
}

Pair c_make_pair(long a, long b) { Pair p; p.a = a; p.b = b; return p; }
long c_pair_sum(Pair p) { return p.a + p.b; }

extern long cx_export(long);
long c_call_corex(long x) { return cx_export(x) + 1; }
