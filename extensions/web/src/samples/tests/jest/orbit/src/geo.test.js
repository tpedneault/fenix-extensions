const dist = (a, b) => Math.abs(a - b)

test('dist is symmetric', () => {
  expect(dist(1, 3)).toBe(dist(3, 1))
})
