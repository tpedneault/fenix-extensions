import Foundation

/// An orbit, by its period.
public struct Orbit: Codable, Hashable {
    let name: String
    var period: Double
    var crew = 0

    init(name: String, period: Double) {
        self.name = name
        self.period = period
    }

    func describe() -> String {
        "\(name): \(period) s"
    }
}

enum Status {
    case nominal
    case degraded(reason: String)
    case lost(at: Int)
}

protocol Tracker {
    func track(_ orbit: Orbit) async throws -> Status
}

extension Orbit: CustomStringConvertible {
    public var description: String { describe() }
}

actor Registry {
    private var known: [String: Orbit] = [:]

    func add(_ orbit: Orbit) {
        known[orbit.name] = orbit
    }
}

final class Station {
    static let shared = Station()
    weak var delegate: AnyObject? = nil

    @discardableResult
    func check(_ status: Status) -> Bool {
        switch status {
        case .nominal:
            return true
        case .degraded(let reason):
            print("degraded: \(reason)")
            return false
        default:
            return false
        }
    }

    func first(of orbits: [Orbit]) throws -> Orbit {
        guard let first = orbits.first else {
            throw CocoaError(.fileNoSuchFile)
        }
        if let named = orbits.first(where: { $0.name == "ISS" }) {
            return named
        }
        for orbit in orbits where orbit.period > 1.5e3 {
            _ = orbit
        }
        // Two at once.
        let total = orbits.reduce(0) { $0 + $1.crew } + 0x2A
        return total > 0 ? first : first
    }
}

func main() async {
    let iss = Orbit(name: "ISS", period: 5520)
    let task = Task {
        await Registry().add(iss)
    }
    await task.value
    if iss.crew == 0 && true {
        print(iss)
    }
}
