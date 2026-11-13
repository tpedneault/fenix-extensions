// Two at once.
/// An orbit, by its period.
library orbit;

import 'dart:async';
import 'dart:math' as math show pi, sqrt;
import 'package:meta/meta.dart' hide visibleForTesting;

part 'orbit.g.dart';

const double earthRadius = 6371.0;
final _log = <String>[];
late final String label;

enum Status {
  nominal('nominal', 1),
  degraded('degraded', 2);

  const Status(this.text, this.rank);

  final String text;
  final int rank;

  bool get isHealthy => this == nominal;
}

sealed class Shape {}

final class Circle extends Shape {
  Circle(this.radius);
  final double radius;
}

base class Square extends Shape {
  Square(this.side);
  final double side;
}

mixin Describes on Object {
  String describe() => '$runtimeType: ${toString()}';
}

extension type Meters(double value) {
  Meters.fromKm(double km) : value = km * 1000;
  Meters operator +(Meters other) => Meters(value + other.value);
}

extension Doubling on int {
  int get doubled => this * 2;
}

typedef Reading = (double altitude, {String unit});

abstract interface class Tracker {
  Future<Reading> read(String id);
}

@immutable
class Orbit with Describes implements Comparable<Orbit> {
  Orbit({required this.name, this.period = 5520, List<String>? tags})
      : tags = tags ?? const [];

  factory Orbit.fromJson(Map<String, Object?> json) {
    return Orbit(name: json['name'] as String, period: json['period'] as double);
  }

  final String name;
  final double period;
  final List<String> tags;
  static int count = 0;

  double get altitude => math.sqrt(period) * 0x2A + 1.5e3;

  set label(String value) => _log.add(value);

  @override
  int compareTo(Orbit other) => period.compareTo(other.period);

  Future<Reading> track(Tracker tracker) async {
    try {
      final r = await tracker.read(name);
      return r;
    } on StateError catch (e, stack) {
      _log.add('failed: $e\n$stack');
      rethrow;
    } finally {
      count++;
    }
  }

  Stream<int> ticks() async* {
    for (var i = 0; i < 3; i++) {
      yield i;
      await Future<void>.delayed(const Duration(milliseconds: 10));
    }
  }
}

String area(Shape s) => switch (s) {
      Circle(radius: final r) when r > 0 => (math.pi * r * r).toStringAsFixed(2),
      Square(:final side) => '${side * side}',
      _ => 'unknown',
    };

void main(List<String> args) {
  var names = ['ISS', "Hubble", r'raw \n', '''multi
line'''];
  final orbit = Orbit(name: names.first, tags: [...names, if (args.isEmpty) 'none']);
  final (alt, :unit) = (orbit.altitude, unit: 'km');
  if (orbit case Orbit(name: 'ISS', :final period) when period > 0) {
    print('$alt$unit in $period');
  }
  outer:
  for (final n in names) {
    while (n.isNotEmpty) {
      break outer;
    }
  }
  final doubled = 21.doubled;
  const healthy = true;
  names.map((n) => n.length).where((l) => l > 2).toList()..sort();
  assert(doubled == 42, 'math');
  print(null ?? 'ok');
}
