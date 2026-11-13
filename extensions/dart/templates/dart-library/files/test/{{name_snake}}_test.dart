import 'package:{{name_snake}}/{{name_snake}}.dart';
import 'package:test/test.dart';

void main() {
  group('Total', () {
    late Total total;

    setUp(() {
      total = Total();
    });

    test('starts at zero', () {
      expect(total.value, 0);
    });

    test('adds, and chains', () {
      expect(total.add(40).add(2).value, 42);
    });
  });
}
