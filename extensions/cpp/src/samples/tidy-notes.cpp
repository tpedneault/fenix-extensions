int leak(int radius) {
    int *p = new int(radius);
    if (radius > 2) return 0;
    return *p;
}
