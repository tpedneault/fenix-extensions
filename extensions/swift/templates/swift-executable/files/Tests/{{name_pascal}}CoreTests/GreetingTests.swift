import Testing
@testable import {{name_pascal}}Core

@Test func greetsTheWorldWhenTheresNoOne() {
    #expect(greeting(for: []) == "Hello, world!")
}

@Test(arguments: [(["Ada"], "Hello, Ada!"), (["Ada", "Grace", "Linus"], "Hello, Ada, Grace and Linus!")])
func greetsEveryone(names: [String], expected: String) {
    #expect(greeting(for: names) == expected)
}
