import { test, expect } from 'vitest'

const dist = (a: number, b: number): number => Math.abs(a - b)

test('dist is symmetric', () => {
  expect(dist(1, 3)).toBe(dist(3, 1))
})
