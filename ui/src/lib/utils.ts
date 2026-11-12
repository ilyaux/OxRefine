export { cn } from "cn"

/** The largest / smallest number in a list of any length (`Math.max(...xs)` overflows the stack past
 * ~100k items, which a 250k-value cluster reaches). */
export const maxOf = (xs: Iterable<number>, start = -Infinity) => {
  let m = start
  for (const x of xs) if (x > m) m = x
  return m
}
export const minOf = (xs: Iterable<number>) => -maxOf([...xs].map((x) => -x))
