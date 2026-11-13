// Fenix's Jest reporter: a "FENIX-TEST {json}" line on stdout for each test
// as it ends (its file, line, names, state, time and failures), those that
// didn't run at their file's end (skipped, todo, or left out by -t: what a
// listing with a -t matching nothing gives), and a file that didn't load or
// whose worker died. Each line has the folder Jest runs in, so the paths in
// it can be made the project's. Given with --reporters=default
// --reporters=<this file> --testLocationInResults.
const cwd = process.cwd()
const say = (o) => process.stdout.write('\nFENIX-TEST ' + JSON.stringify({ ...o, cwd }) + '\n')
const cut = (s) => String(s ?? '').slice(0, 4000)

class FenixReporter {
  onTestCaseResult(test, r) {
    say({ kind: 'result', file: test.path, names: [...r.ancestorTitles, r.title], line: r.location?.line ?? null, state: r.status, duration: r.duration ?? null, failures: r.failureMessages.map(cut) })
  }
  onTestResult(test, result) {
    for (const r of result.testResults) {
      if (r.status === 'pending' || r.status === 'skipped' || r.status === 'todo' || r.status === 'disabled') {
        say({ kind: 'test', file: test.path, names: [...r.ancestorTitles, r.title], line: r.location?.line ?? null, state: r.status })
      }
    }
    if (result.testExecError) say({ kind: 'file-error', file: test.path, message: cut(result.testExecError.message), stack: cut(result.testExecError.stack) })
  }
}
module.exports = FenixReporter
