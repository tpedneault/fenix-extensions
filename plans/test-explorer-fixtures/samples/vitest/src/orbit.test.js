import { describe, it, test, expect } from 'vitest'
import { add } from './orbit.js'

test('adds', () => {
  expect(add(1, 2)).toBe(3)
})

test('fails on purpose', () => {
  console.log('some output before the failure')
  expect(add(2, 2)).toBe(5)
})

test.skip('skipped', () => {})
test.todo('to do')

test.each([
  [1, 1, 2],
  [-1, -1, -3],
])('add(%i, %i) is %i', (a, b, want) => {
  expect(add(a, b)).toBe(want)
})

describe('outer', () => {
  it('in describe', () => {})
  describe('inner', () => {
    it('deep', () => {
      expect(add(0, 0)).toBe(1)
    })
  })
})
