import 'dart:io';

import 'package:args/args.dart';
import 'package:{{name_snake}}/{{name_snake}}.dart';

Future<void> main(List<String> arguments) async {
  final parser = ArgParser()
    ..addFlag('shout', abbr: 's', negatable: false, help: 'Greet loudly.')
    ..addFlag('help', abbr: 'h', negatable: false, help: 'Show this help.');
  final ArgResults results;
  try {
    results = parser.parse(arguments);
  } on FormatException catch (e) {
    stderr.writeln('{{name_kebab}}: ${e.message}');
    exitCode = 64;
    return;
  }
  if (results.flag('help')) {
    stdout.writeln('Usage: {{name_kebab}} [--shout] [name...]\n${parser.usage}');
    return;
  }
  final names = results.rest.isEmpty ? ['world'] : results.rest;
  for (final name in names) {
    stdout.writeln(greeting(name, shout: results.flag('shout')));
  }
}
