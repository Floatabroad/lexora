import random
import re
import subprocess
import sys
import os

TOKEN = re.compile(r"\s+|\w+|.")
STRAY = ["{", "}", "(", ")", ";", ",", "=>", "::", "@", "?", "]", "["]


def mutate(src, rng):
    toks = TOKEN.findall(src)
    idx = [i for i, t in enumerate(toks) if t.strip()]
    if not idx:
        return src
    kind = rng.choice(["del", "dup", "swap", "stray"])
    for _ in range(rng.randint(1, 3)):
        i = rng.choice(idx)
        if kind == "del":
            toks[i] = ""
        elif kind == "dup":
            toks[i] = toks[i] + toks[i]
        elif kind == "swap":
            j = rng.choice(idx)
            toks[i], toks[j] = toks[j], toks[i]
        else:
            toks[i] = rng.choice(STRAY)
    return "".join(toks)


if __name__ == "__main__":
    seed = int(sys.argv[1])
    here = os.path.dirname(os.path.abspath(__file__))
    base = subprocess.run(
        [sys.executable, os.path.join(here, "fuzz_gen.py"), str(seed)],
        capture_output=True,
        text=True,
    ).stdout
    print(mutate(base, random.Random(seed)), end="")
