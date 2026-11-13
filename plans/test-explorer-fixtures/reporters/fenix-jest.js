// What the test explorer asks of Jest: each test as it ends, a JSON line
// starting FENIX-TEST on stdout -- its file, line, names, state, time and
// failure. Given with --reporters=default --reporters=<this file>
// --testLocationInResults.
const say = (o) => process.stdout.write('\nFENIX-TEST ' + JSON.stringify(o) + '\n')

class FenixReporter {
  onTestCaseResult(test, r) {
    say({ kind: 'result', file: test.path, names: [...r.ancestorTitles, r.title], line: r.location?.line ?? null, state: r.status, duration: r.duration ?? null, failures: r.failureMessages.map((m) => m.slice(0, 4000)) })
  }
  onTestResult(test, result) {
    // Those that didn't run -- skipped, todo, or left out by -t -- come only
    // here, at the file's end: what a discovery run (-t matching nothing) lists.
    for (const r of result.testResults) {
      if (r.status === 'pending' || r.status === 'skipped' || r.status === 'todo' || r.status === 'disabled') {
        say({ kind: 'test', file: test.path, names: [...r.ancestorTitles, r.title], line: r.location?.line ?? null, state: r.status })
      }
    }
    if (result.testExecError) say({ kind: 'file-error', file: test.path, message: String(result.testExecError.message).slice(0, 4000) })
  }
}
module.exports = FenixReporter
