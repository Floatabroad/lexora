import subprocess
import sys
import os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "debug", "lexora")

PRE = """enum Opt<T> {
    Some(T),
    None,
}

enum K {
    KA(i32),
    KB,
}

enum M {
    MA(String),
    MB,
}

enum N {
    NA(Box<i32>),
    NB,
}

enum U {
    UA,
    UB,
}

struct P {
    x: i32,
}

struct Q {
    s: String,
}

struct R {
    b: Box<i32>,
}
"""

BASE = [
    "i32", "i64", "bool", "str", "String",
    "K", "M", "N", "U", "P", "Q", "R",
    "Opt<i32>", "Opt<String>", "Opt<Box<i32>>",
    "Opt<K>", "Opt<M>", "Opt<P>", "Opt<Q>",
]

POSITIONS = {
    "param": "fn f(x: %s) -> i32 {\n    return 0;\n}\n",
    "field": "struct Z {\n    z: %s,\n}\n\nfn f(x: Z) -> i32 {\n    return 0;\n}\n",
    "ret": "fn f(x: i32) -> %s {\n    return g();\n}\n\nfn g() -> %s {\n    return h();\n}\n",
    "boxret": "fn f(x: i32) -> Box<%s> {\n    return g();\n}\n\nfn g() -> Box<%s> {\n    return f(1);\n}\n",
}

RECURSIVE = [
    ("red", "struct S {\n    b: Box<S>,\n}\n\nfn f(x: S) -> i32 {\n    return 0;\n}\n"),
    ("red", "struct A {\n    b: Box<B>,\n}\n\nstruct B {\n    a: Box<A>,\n}\n\nfn f(x: A) -> i32 {\n    return 0;\n}\n"),
    ("red", "struct D {\n    a: [Box<D>; 2],\n}\n\nfn f(x: D) -> i32 {\n    return 0;\n}\n"),
    ("red", "struct E {\n    d: E,\n}\n\nfn f(x: E) -> i32 {\n    return 0;\n}\n"),
    ("kabul", "struct Node {\n    val: i32,\n    next: Opt<Box<Node>>,\n}\n\nfn f(x: Node) -> i32 {\n    return x.val;\n}\n"),
    ("kabul", "struct Kutu {\n    ad: String,\n    ic: Opt<Box<Kutu>>,\n}\n\nfn f(x: Kutu) -> i32 {\n    return len(x.ad);\n}\n"),
]


def shapes():
    out = []
    for b in BASE:
        out.append(b)
        out.append("Box<%s>" % b)
        out.append("[%s; 2]" % b)
        out.append("Box<[%s; 2]>" % b)
        out.append("[Box<%s>; 2]" % b)
    return out


def run(src, tmp):
    path = os.path.join(tmp, "ts.lx")
    with open(path, "w") as fh:
        fh.write(src)
    results = []
    for backend in ("string-ir", "inkwell"):
        cmd = "ulimit -v 4000000; timeout 30 %s --backend %s %s" % (BIN, backend, path)
        proc = subprocess.run(["bash", "-c", cmd], capture_output=True, text=True)
        err = (proc.stdout or "") + (proc.stderr or "")
        results.append((backend, proc.returncode, err))
    return results


def bad(rc, err):
    if rc not in (0, 1):
        return "cikis kodu %s (0 veya 1 olmali)" % rc
    if "panicked" in err:
        return "DERLEYICI PANIC"
    if "overflowed its stack" in err:
        return "STACK OVERFLOW"
    return None


def main():
    tmp = sys.argv[1]
    runs = 0
    fails = 0
    shs = shapes()

    for pos, tmpl in POSITIONS.items():
        for shape in shs:
            src = PRE + "\n" + (tmpl % ((shape,) * tmpl.count("%s")))
            src += "\nfn main() -> i32 {\n    return 0;\n}\n"
            for backend, rc, err in run(src, tmp):
                runs += 1
                why = bad(rc, err)
                if why:
                    fails += 1
                    print("FAIL  %-8s %-24s %-10s %s" % (pos, shape, backend, why))

    for beklenen, body in RECURSIVE:
        src = PRE + "\n" + body + "\nfn main() -> i32 {\n    return 0;\n}\n"
        for backend, rc, err in run(src, tmp):
            runs += 1
            why = bad(rc, err)
            if why is None and beklenen == "red" and rc != 1:
                why = "temiz redde olmaliydi (rc=%s)" % rc
            if why is None and beklenen == "kabul" and rc != 0:
                why = "derlenmeliydi (rc=%s): %s" % (rc, err.strip().splitlines()[0][:60] if err.strip() else "")
            if why:
                fails += 1
                print("FAIL  ozyineleme/%-6s %-10s %s" % (beklenen, backend, why))

    print("------------------------------------------")
    print("tip supurmesi: %d kosum (%d sekil x %d tip-pozisyonu + %d ozyineleme, x 2 backend)"
          % (runs, len(shs), len(POSITIONS), len(RECURSIVE)))
    print("  BASARISIZ: %d" % fails)
    if fails:
        return 1
    print("tip supurmesi: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
