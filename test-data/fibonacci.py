"""Fibonacci sequence generators — several approaches compared."""

from functools import lru_cache
from typing import Generator, Iterator
import time

# Naive recursive (exponential time)
def fib_naive(n: int) -> int:
    if n <= 1:
        return n
    return fib_naive(n - 1) + fib_naive(n - 2)

# Memoized recursive (linear time, linear space)
@lru_cache(maxsize=None)
def fib_memo(n: int) -> int:
    if n <= 1:
        return n
    return fib_memo(n - 1) + fib_memo(n - 2)

# Iterative (linear time, constant space)
def fib_iter(n: int) -> int:
    a, b = 0, 1
    for _ in range(n):
        a, b = b, a + b
    return a

# Generator (lazy infinite sequence)
def fib_gen() -> Generator[int, None, None]:
    a, b = 0, 1
    while True:
        yield a
        a, b = b, a + b

# Matrix exponentiation (logarithmic time)
def fib_matrix(n: int) -> int:
    def multiply(a: list, b: list) -> list:
        return [
            [a[0][0]*b[0][0] + a[0][1]*b[1][0], a[0][0]*b[0][1] + a[0][1]*b[1][1]],
            [a[1][0]*b[0][0] + a[1][1]*b[1][0], a[1][0]*b[0][1] + a[1][1]*b[1][1]],
        ]

    def power(mat: list, p: int) -> list:
        if p == 1:
            return mat
        if p % 2 == 0:
            half = power(mat, p // 2)
            return multiply(half, half)
        return multiply(mat, power(mat, p - 1))

    if n <= 1:
        return n
    result = power([[1, 1], [1, 0]], n)
    return result[0][1]


def benchmark(name: str, func, n: int, iterations: int = 1000) -> float:
    """Time a fibonacci function over multiple iterations."""
    start = time.perf_counter()
    for _ in range(iterations):
        func(n)
    elapsed = time.perf_counter() - start
    avg_us = (elapsed / iterations) * 1_000_000
    print(f"  {name:<20s} fib({n}) = {func(n):<12d} avg {avg_us:>8.2f} µs")
    return avg_us


if __name__ == "__main__":
    N = 30
    print(f"Fibonacci benchmarks (n={N}):\n")

    benchmark("naive", fib_naive, N, iterations=10)
    fib_memo.cache_clear()
    benchmark("memoized", fib_memo, N)
    benchmark("iterative", fib_iter, N)
    benchmark("matrix", fib_matrix, N)

    print(f"\nFirst 20 values (generator):")
    gen = fib_gen()
    values = [next(gen) for _ in range(20)]
    print(f"  {values}")
