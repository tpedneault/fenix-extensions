/// A running total of the numbers it has been given.
class Total {
  num _sum = 0;

  /// What's been added so far.
  num get value => _sum;

  /// Adds `amount`, and returns this, so adds can be chained.
  Total add(num amount) {
    _sum += amount;
    return this;
  }
}
