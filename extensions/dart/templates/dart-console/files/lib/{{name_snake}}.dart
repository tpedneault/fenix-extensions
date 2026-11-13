/// What {{name_kebab}} says to `name`; `shout` says it loudly.
String greeting(String name, {bool shout = false}) {
  final line = 'Hello, ${name.trim()}!';
  return shout ? line.toUpperCase() : line;
}
