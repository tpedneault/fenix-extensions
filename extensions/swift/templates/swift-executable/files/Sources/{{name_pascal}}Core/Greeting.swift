/// A greeting for everyone in `names`, or the world when there's no one.
public func greeting(for names: [String]) -> String {
    switch names.count {
    case 0:
        return "Hello, world!"
    case 1:
        return "Hello, \(names[0])!"
    default:
        return "Hello, \(names.dropLast().joined(separator: ", ")) and \(names.last!)!"
    }
}
