// Fenix's Vitest reporter (Vitest 3 and later): a "FENIX-TEST {json}" line
// on stdout for each test collected (skip, todo and each case of test.each
// included), each result as it ends with its errors, what a test logged,
// a file that didn't load and an error outside any test. Each line has the
// folder Vitest runs in, so the paths in it can be made the project's.
// Given with --reporter=default --reporter=<this file> --includeTaskLocation.
const cwd = process.cwd()
const say = (o) => process.stdout.write('\nFENIX-TEST ' + JSON.stringify({ ...o, cwd }) + '\n')
const cut = (s) => (typeof s === 'string' ? s.slice(0, 4000) : undefined)
const error = (e) => ({ message: cut(String(e?.message ?? e)), stack: cut(e?.stack), diff: cut(e?.diff), cause: cut(e?.cause?.message) })

export default class FenixReporter {
  onInit(vitest) {
    this.vitest = vitest
  }
  onTestModuleCollected(module) {
    for (const t of module.children.allTests()) {
      say({ kind: 'test', file: module.moduleId, names: names(t), line: t.location?.line ?? null, column: t.location?.column ?? null, mode: t.options.mode })
    }
  }
  onTestCaseResult(t) {
    const r = t.result()
    const d = t.diagnostic()
    say({ kind: 'result', file: t.module.moduleId, names: names(t), line: t.location?.line ?? null, state: r.state, duration: d?.duration ?? null, errors: (r.errors ?? []).map(error) })
  }
  onUserConsoleLog(log) {
    const task = log.taskId && this.vitest?.state?.idMap?.get(log.taskId)
    const t = task && this.vitest.state.getReportedEntity?.(task)
    if (t && t.type === 'test') say({ kind: 'log', file: t.module.moduleId, names: names(t), stream: log.type, text: cut(log.content) })
  }
  onTestModuleEnd(module) {
    const errors = module.errors()
    if (errors.length) say({ kind: 'module-error', file: module.moduleId, errors: errors.map(error) })
  }
  onTestRunEnd(_modules, unhandled) {
    for (const e of unhandled ?? []) say({ kind: 'run-error', error: error(e) })
  }
}

function names(t) {
  const out = [t.name]
  for (let p = t.parent; p && p.type === 'suite'; p = p.parent) out.unshift(p.name)
  return out
}
