#include <cstddef>

struct Base {
    virtual ~Base() = default;
    virtual int radius() const { return 1; }
};

struct Moon : Base {
    virtual int radius() const { return 2; }
};

int orbit(int *p) {
    if (p == NULL) return 0;
    int r = *p;
    return r;
}
