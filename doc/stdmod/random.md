# Random module

`Random` provides integer random helpers over half-open ranges.

## Surface

- `Random::seed(seed: Int) -> RandomGenerator`
- `Random::int_until(end: Int) -> Result<Int, InvalidRandomRange>`
- `Random::int_range(start: Int, end: Int) -> Result<Int, InvalidRandomRange>`
- `Random::next_int_until(rng: RandomGenerator, end: Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>`
- `Random::next_int_range(rng: RandomGenerator, start: Int, end: Int) -> Result<(Int, RandomGenerator), InvalidRandomRange>`

`Random::int_*` uses host-provided entropy for each call.
`Random::next_*` returns the next generator state explicitly so seeded
sequences can be threaded through pure-looking Surtr values.

## Error contract

Ranges are half-open. `start >= end` returns
`Err(InvalidRandomRange(start, end))`; `int_until(end)` treats the start as `0`.

`RandomGenerator` display is intentionally opaque, and the exact PRNG algorithm
is not a surface-language contract.
