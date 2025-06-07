import asyncio
import datetime as dt
import random
import time
from typing import Callable

from rustflight import PyCache

cache = PyCache(timeout=5000)  # Wait 5 seconds


def call_with_cache(func: Callable):
    def wrapper(*args, **kwargs):
        entry_key = str(
            hash(args + tuple(kwargs.items()))
        )  # -> Arbitrary hashing logic
        result = cache.py_call(func, args, kwargs, entry_key)
        return result

    return wrapper


@call_with_cache
def get_random_number_after_seconds(lower, upper, seconds, **kwargs) -> int:
    print("Starting sleep...")
    time.sleep(seconds)
    print("Sleep ended!")
    number = random.randint(lower, upper)
    return number * kwargs.get("multiplier", 1)


async def run_in_parallel():
    start = dt.datetime.now()

    # Launch both calls concurrently
    results = await asyncio.gather(
        asyncio.to_thread(get_random_number_after_seconds, 1, 10, 1, multiplier=100),
        asyncio.to_thread(get_random_number_after_seconds, 1, 10, 1, multiplier=100),
    )

    time_for_first_two_calls = dt.datetime.now()
    third_number = get_random_number_after_seconds(1, 10, 1, multiplier=100)
    end = dt.datetime.now()

    # Assertions and results
    assert results[0] == results[1] == third_number, "Unexpected: %s, %s, %s" % (results[0], results[1], third_number)

    print("Time for first two calls: %ss" % (time_for_first_two_calls - start).seconds)
    print("Total time: %ss" % (end - start).seconds)


if __name__ == "__main__":
    asyncio.run(run_in_parallel())
