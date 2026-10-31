import {{name_pascal}}Core

@main
struct {{name_pascal}} {
    static func main() {
        let names = Array(CommandLine.arguments.dropFirst())
        print(greeting(for: names))
    }
}
