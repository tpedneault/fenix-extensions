import 'package:{{name_snake}}/{{name_snake}}.dart';
import 'package:test/test.dart';

void main() {
  group('greeting', () {
    test('greets by name', () {
      expect(greeting('Ada'), 'Hello, Ada!');
    });

    test('trims the name', () {
      expect(greeting('  Grace '), 'Hello, Grace!');
    });

    test('shouts when asked', () {
      expect(greeting('Rob', shout: true), 'HELLO, ROB!');
    });
  });
}
