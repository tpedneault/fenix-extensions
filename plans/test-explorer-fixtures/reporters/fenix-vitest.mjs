// What the test explorer asks of Vitest: each test as it ends, a JSON line
// starting FENIX-TEST on stdout -- its file, line, names, state, time and
// errors. Given with --reporter=default --reporter=<this file>.
const say = (o) => process.stdout.write('\nFENIX-TEST ' + JSON.stringify(o) + '\n')

export default class FenixReporter {
  onTestModuleCollected(module) {
    for (const t of module.children.allTests()) {
      say({ kind: 'test', file: module.moduleId, names: names(t), line: t.location?.line ?? null, column: t.location?.column ?? null, mode: t.options.mode })
    }
  }
  onTestCaseResult(t) {
    const r = t.result()
    const d = t.diagnostic()
    say({
      kind: 'result', file: t.module.moduleId, names: names(t), line: t.location?.line ?? null,
      state: r.state, duration: d?.duration ?? null,
      errors: (r.errors ?? []).map((e) => ({ message: e.message, stack: e.stack?.slice(0, 4000), diff: e.diff?.slice(0, 4000) })),
    })
  }
  onTestModuleEnd(module) {
    const errors = module.errors()
    if (errors.length) say({ kind: 'module-error', file: module.moduleId, errors: errors.map((e) => ({ message: e.message, stack: e.stack?.slice(0, 4000) })) })
  }
}

function names(t) {
  const out = [t.name]
  for (let p = t.parent; p && p.type === 'suite'; p = p.parent) out.unshift(p.name)
  return out
}
